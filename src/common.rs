use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    sync::{Arc, Mutex, RwLock},
    task::Poll,
};

use serde_json::{json, Map, Value};

use base::{config::keys, message_proto::*};
#[cfg(not(target_os = "ios"))]
use hbb_common::whoami;
use hbb_common::{
    allow_err,
    anyhow::{anyhow, Context},
    async_recursion::async_recursion,
    bail, base64,
    bytes::Bytes,
    config::{self, Config, LocalConfig, CONNECT_TIMEOUT, READ_TIMEOUT},
    futures::future::join_all,
    futures_util::future::poll_fn,
    get_version_number, log,
    protobuf::{Enum, Message as _},
    rendezvous_proto::*,
    socket_client,
    sodiumoxide::crypto::{box_, secretbox, sign},
    timeout,
    tokio::{
        self,
        net::UdpSocket,
        time::{Duration, Instant, Interval},
    },
    ResultType, Stream,
};

use crate::{
    ui_interface::{get_option, set_option},
};

#[derive(Debug, Eq, PartialEq)]
pub enum GrabState {
    Ready,
    Run,
    Wait,
    Exit,
}

pub type NotifyMessageBox = fn(String, String, String, String) -> dyn Future<Output = ()>;

// the executable name of the portable version
pub const PORTABLE_APPNAME_RUNTIME_ENV_KEY: &str = "RUSTDESK_APPNAME";

pub const PLATFORM_WINDOWS: &str = "Windows";
pub const PLATFORM_LINUX: &str = "Linux";
pub const PLATFORM_MACOS: &str = "Mac OS";
pub const PLATFORM_ANDROID: &str = "Android";

pub const TIMER_OUT: Duration = Duration::from_secs(1);
pub(crate) const API_LOG_INTERVAL: Duration = Duration::from_secs(600);
pub const DEFAULT_KEEP_ALIVE: i32 = 60_000;

const MIN_VER_MULTI_UI_SESSION: &str = "1.2.4";

pub mod input {
    pub const MOUSE_TYPE_MOVE: i32 = 0;
    pub const MOUSE_TYPE_DOWN: i32 = 1;
    pub const MOUSE_TYPE_UP: i32 = 2;
    pub const MOUSE_TYPE_WHEEL: i32 = 3;
    pub const MOUSE_TYPE_TRACKPAD: i32 = 4;
    /// Relative mouse movement type for gaming/3D applications.
    /// This type sends delta (dx, dy) values instead of absolute coordinates.
    pub const MOUSE_TYPE_MOVE_RELATIVE: i32 = 5;

    /// Mask to extract the mouse event type from the mask field.
    /// The lower 3 bits contain the event type (MOUSE_TYPE_*), giving a valid range of 0-7.
    /// Currently defined types use values 0-5; values 6 and 7 are reserved for future use.
    pub const MOUSE_TYPE_MASK: i32 = 0x7;

    pub const MOUSE_BUTTON_LEFT: i32 = 0x01;
    pub const MOUSE_BUTTON_RIGHT: i32 = 0x02;
    pub const MOUSE_BUTTON_WHEEL: i32 = 0x04;
    pub const MOUSE_BUTTON_BACK: i32 = 0x08;
    pub const MOUSE_BUTTON_FORWARD: i32 = 0x10;
}

lazy_static::lazy_static! {
    pub static ref DEVICE_ID: Arc<Mutex<String>> = Default::default();
    pub static ref DEVICE_NAME: Arc<Mutex<String>> = Default::default();
    static ref PUBLIC_IPV6_ADDR: Arc<Mutex<(Option<SocketAddr>, Option<Instant>)>> = Default::default();
}

lazy_static::lazy_static! {
    // Is server process, with "--server" args
    static ref IS_SERVER: bool = std::env::args().nth(1) == Some("--server".to_owned());
    // Is server logic running. The server code can invoked to run by the main process if --server is not running.
    static ref SERVER_RUNNING: Arc<RwLock<bool>> = Default::default();
    static ref IS_MAIN: bool = std::env::args().nth(1).map_or(true, |arg| !arg.starts_with("--"));
    static ref IS_CM: bool = std::env::args().nth(1) == Some("--cm".to_owned());
}

pub struct SimpleCallOnReturn {
    pub b: bool,
    pub f: Box<dyn Fn() + Send + 'static>,
}

impl Drop for SimpleCallOnReturn {
    fn drop(&mut self) {
        if self.b {
            (self.f)();
        }
    }
}

pub fn global_init() -> bool {
    #[cfg(all(target_os = "linux", feature = "drm"))]
    crate::platform::linux::dispatch_wayland_display_probe();
    #[cfg(target_os = "linux")]
    {
        if !crate::platform::linux::is_x11() {
            crate::server::wayland::init();
        }
    }
    true
}

pub fn global_clean() {}

#[inline]
pub fn set_server_running(b: bool) {
    *SERVER_RUNNING.write().unwrap() = b;
}

#[inline]
pub fn is_support_multi_ui_session(ver: &str) -> bool {
    is_support_multi_ui_session_num(hbb_common::get_version_number(ver))
}

#[inline]
pub fn is_support_multi_ui_session_num(ver: i64) -> bool {
    ver >= hbb_common::get_version_number(MIN_VER_MULTI_UI_SESSION)
}

/// Peers from the 1.5.0 release name a cursor by `cursor_content_id`; older ones by the
/// platform handle, which apps mint anew for the same shape.
#[inline]
pub fn is_peer_naming_cursors_by_content(ver: i64) -> bool {
    ver >= hbb_common::get_version_number("1.5.0")
}

/// One id per cursor look, however many handles a platform gives it. Held to 53 bits: the web
/// client decodes a u64 into a JS number and drops the whole message when it does not fit.
pub fn cursor_content_id(width: i32, height: i32, hotx: i32, hoty: i32, colors: &[u8]) -> u64 {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    for v in [width, height, hotx, hoty] {
        hasher.update(&v.to_le_bytes());
    }
    hasher.update(colors);
    hasher.digest() & ((1 << 53) - 1)
}

#[inline]
#[cfg(feature = "unix-file-copy-paste")]
pub fn is_support_file_copy_paste(ver: &str) -> bool {
    is_support_file_copy_paste_num(hbb_common::get_version_number(ver))
}

#[inline]
#[cfg(feature = "unix-file-copy-paste")]
pub fn is_support_file_copy_paste_num(ver: i64) -> bool {
    ver >= hbb_common::get_version_number("1.3.8")
}

pub fn is_support_remote_print(ver: &str) -> bool {
    hbb_common::get_version_number(ver) >= hbb_common::get_version_number("1.3.9")
}

pub fn is_support_file_paste_if_macos(ver: &str) -> bool {
    hbb_common::get_version_number(ver) >= hbb_common::get_version_number("1.3.9")
}

#[inline]
pub fn is_support_screenshot(ver: &str) -> bool {
    is_support_multi_ui_session_num(hbb_common::get_version_number(ver))
}

#[inline]
pub fn is_support_screenshot_num(ver: i64) -> bool {
    ver >= hbb_common::get_version_number("1.4.0")
}

#[inline]
pub fn is_support_file_transfer_resume(ver: &str) -> bool {
    is_support_file_transfer_resume_num(hbb_common::get_version_number(ver))
}

#[inline]
pub fn is_support_file_transfer_resume_num(ver: i64) -> bool {
    ver >= hbb_common::get_version_number("1.4.2")
}

/// Minimum server version required for relative mouse mode support.
/// This constant must mirror Flutter's `kMinVersionForRelativeMouseMode` in `consts.dart`.
const MIN_VERSION_RELATIVE_MOUSE_MODE: &str = "1.4.5";

#[inline]
pub fn is_support_relative_mouse_mode(ver: &str) -> bool {
    is_support_relative_mouse_mode_num(hbb_common::get_version_number(ver))
}

#[inline]
pub fn is_support_relative_mouse_mode_num(ver: i64) -> bool {
    ver >= hbb_common::get_version_number(MIN_VERSION_RELATIVE_MOUSE_MODE)
}

// is server process, with "--server" args
#[inline]
pub fn is_server() -> bool {
    *IS_SERVER
}

#[inline]
pub fn need_fs_cm_send_files() -> bool {
    #[cfg(windows)]
    {
        is_server()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Android is scoped-storage only: the peer may never touch anything outside the app
/// workspace (`Config::get_home()`, i.e. the app-specific external files directory).
///
/// Every peer supplied path must be validated with this before it reaches the
/// filesystem, for reads, writes, renames, creations and deletions alike. The path is
/// resolved to its canonical form (of the deepest existing ancestor, so paths that are
/// about to be created are handled too) so symlinks cannot escape the workspace.
///
/// Only the `ReadDir` protocol action treats an empty path as the home directory.
/// Callers must opt in to that protocol-specific behavior with `allow_empty`.
#[cfg(target_os = "android")]
pub fn is_peer_path_allowed(path: &str, allow_empty: bool) -> bool {
    use std::path::{Component, Path, PathBuf};

    // Canonicalize the deepest existing ancestor and re-append the missing tail.
    fn resolve(path: &Path) -> Option<PathBuf> {
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        let mut base = path.to_path_buf();
        loop {
            if let Ok(mut resolved) = base.canonicalize() {
                while let Some(component) = tail.pop() {
                    resolved.push(component);
                }
                return Some(resolved);
            }
            tail.push(base.file_name()?.to_os_string());
            if !base.pop() {
                return None;
            }
        }
    }

    if path.is_empty() {
        return allow_empty;
    }
    let path = Path::new(path);
    // `..` is never needed by the protocol and would defeat the prefix check below.
    if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
        return false;
    }
    let home = Config::get_home();
    let home = home.canonicalize().unwrap_or(home);
    if home.as_os_str().is_empty() {
        return false;
    }
    // `Path::starts_with` compares whole components, and is true for equal paths.
    resolve(path).map_or(false, |target| target.starts_with(&home))
}

#[inline]
#[cfg(not(target_os = "android"))]
pub fn is_peer_path_allowed(_path: &str, _allow_empty: bool) -> bool {
    true
}

#[inline]
pub fn is_main() -> bool {
    *IS_MAIN
}

#[inline]
pub fn is_cm() -> bool {
    *IS_CM
}

// Is server logic running.
#[inline]
pub fn is_server_running() -> bool {
    *SERVER_RUNNING.read().unwrap()
}

#[inline]
pub fn valid_for_numlock(evt: &KeyEvent) -> bool {
    if let Some(key_event::Union::ControlKey(ck)) = evt.union {
        let v = ck.value();
        (v >= ControlKey::Numpad0.value() && v <= ControlKey::Numpad9.value())
            || v == ControlKey::Decimal.value()
    } else {
        false
    }
}

/// Set sound input device.
pub fn set_sound_input(device: String) {
    let prior_device = get_option("audio-input".to_owned());
    if prior_device != device {
        log::info!("switch to audio input device {}", device);
        std::thread::spawn(move || {
            set_option("audio-input".to_owned(), device);
        });
    } else {
        log::info!("audio input is already set to {}", device);
    }
}

/// Get system's default sound input device name.
#[inline]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn get_default_sound_input() -> Option<String> {
    #[cfg(not(target_os = "linux"))]
    {
        use cpal::traits::{DeviceTrait, HostTrait};
        let host = cpal::default_host();
        let dev = host.default_input_device();
        return if let Some(dev) = dev {
            match dev.name() {
                Ok(name) => Some(name),
                Err(_) => None,
            }
        } else {
            None
        };
    }
    #[cfg(target_os = "linux")]
    {
        let input = crate::platform::linux::get_default_pa_source();
        return if let Some(input) = input {
            Some(input.1)
        } else {
            None
        };
    }
}

#[inline]
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn get_default_sound_input() -> Option<String> {
    None
}

#[cfg(feature = "use_rubato")]
pub fn resample_channels(
    data: &[f32],
    sample_rate0: u32,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    use rubato::{
        InterpolationParameters, InterpolationType, Resampler, SincFixedIn, WindowFunction,
    };
    let params = InterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: InterpolationType::Nearest,
        oversampling_factor: 160,
        window: WindowFunction::BlackmanHarris2,
    };
    let mut resampler = SincFixedIn::<f64>::new(
        sample_rate as f64 / sample_rate0 as f64,
        params,
        data.len() / (channels as usize),
        channels as _,
    );
    let mut waves_in = Vec::new();
    if channels == 2 {
        waves_in.push(
            data.iter()
                .step_by(2)
                .map(|x| *x as f64)
                .collect::<Vec<_>>(),
        );
        waves_in.push(
            data.iter()
                .skip(1)
                .step_by(2)
                .map(|x| *x as f64)
                .collect::<Vec<_>>(),
        );
    } else {
        waves_in.push(data.iter().map(|x| *x as f64).collect::<Vec<_>>());
    }
    if let Ok(x) = resampler.process(&waves_in) {
        if x.is_empty() {
            Vec::new()
        } else if x.len() == 2 {
            x[0].chunks(1)
                .zip(x[1].chunks(1))
                .flat_map(|(a, b)| a.into_iter().chain(b))
                .map(|x| *x as f32)
                .collect()
        } else {
            x[0].iter().map(|x| *x as f32).collect()
        }
    } else {
        Vec::new()
    }
}

#[cfg(all(feature = "use_dasp", feature = "use_samplerate"))]
compile_error!(
    "features `use_dasp` and `use_samplerate` are mutually exclusive; disable default features before selecting `use_samplerate`"
);

#[cfg(feature = "use_dasp")]
pub fn audio_resample(
    data: &[f32],
    sample_rate0: u32,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    use dasp::{interpolate::linear::Linear, signal, Signal};
    let n = data.len() / (channels as usize);
    let n = n * sample_rate as usize / sample_rate0 as usize;
    if channels == 2 {
        let mut source = signal::from_interleaved_samples_iter::<_, [_; 2]>(data.iter().cloned());
        let a = source.next();
        let b = source.next();
        let interp = Linear::new(a, b);
        let mut data = Vec::with_capacity(n << 1);
        for x in source
            .from_hz_to_hz(interp, sample_rate0 as _, sample_rate as _)
            .take(n)
        {
            data.push(x[0]);
            data.push(x[1]);
        }
        data
    } else {
        let mut source = signal::from_iter(data.iter().cloned());
        let a = source.next();
        let b = source.next();
        let interp = Linear::new(a, b);
        source
            .from_hz_to_hz(interp, sample_rate0 as _, sample_rate as _)
            .take(n)
            .collect()
    }
}

#[cfg(all(feature = "use_samplerate", not(feature = "use_dasp")))]
pub fn audio_resample(
    data: &[f32],
    sample_rate0: u32,
    sample_rate: u32,
    channels: u16,
) -> Vec<f32> {
    use samplerate::{convert, ConverterType};
    convert(
        sample_rate0 as _,
        sample_rate as _,
        channels as _,
        ConverterType::SincBestQuality,
        data,
    )
    .unwrap_or_default()
}

pub fn audio_rechannel(
    input: Vec<f32>,
    in_hz: u32,
    out_hz: u32,
    in_chan: u16,
    output_chan: u16,
) -> Vec<f32> {
    if in_chan == output_chan {
        return input;
    }
    let mut input = input;
    input.truncate(input.len() / in_chan as usize * in_chan as usize);
    match (in_chan, output_chan) {
        (1, 2) => audio_rechannel_1_2(&input, in_hz, out_hz),
        (1, 3) => audio_rechannel_1_3(&input, in_hz, out_hz),
        (1, 4) => audio_rechannel_1_4(&input, in_hz, out_hz),
        (1, 5) => audio_rechannel_1_5(&input, in_hz, out_hz),
        (1, 6) => audio_rechannel_1_6(&input, in_hz, out_hz),
        (1, 7) => audio_rechannel_1_7(&input, in_hz, out_hz),
        (1, 8) => audio_rechannel_1_8(&input, in_hz, out_hz),
        (2, 1) => audio_rechannel_2_1(&input, in_hz, out_hz),
        (2, 3) => audio_rechannel_2_3(&input, in_hz, out_hz),
        (2, 4) => audio_rechannel_2_4(&input, in_hz, out_hz),
        (2, 5) => audio_rechannel_2_5(&input, in_hz, out_hz),
        (2, 6) => audio_rechannel_2_6(&input, in_hz, out_hz),
        (2, 7) => audio_rechannel_2_7(&input, in_hz, out_hz),
        (2, 8) => audio_rechannel_2_8(&input, in_hz, out_hz),
        (3, 1) => audio_rechannel_3_1(&input, in_hz, out_hz),
        (3, 2) => audio_rechannel_3_2(&input, in_hz, out_hz),
        (3, 4) => audio_rechannel_3_4(&input, in_hz, out_hz),
        (3, 5) => audio_rechannel_3_5(&input, in_hz, out_hz),
        (3, 6) => audio_rechannel_3_6(&input, in_hz, out_hz),
        (3, 7) => audio_rechannel_3_7(&input, in_hz, out_hz),
        (3, 8) => audio_rechannel_3_8(&input, in_hz, out_hz),
        (4, 1) => audio_rechannel_4_1(&input, in_hz, out_hz),
        (4, 2) => audio_rechannel_4_2(&input, in_hz, out_hz),
        (4, 3) => audio_rechannel_4_3(&input, in_hz, out_hz),
        (4, 5) => audio_rechannel_4_5(&input, in_hz, out_hz),
        (4, 6) => audio_rechannel_4_6(&input, in_hz, out_hz),
        (4, 7) => audio_rechannel_4_7(&input, in_hz, out_hz),
        (4, 8) => audio_rechannel_4_8(&input, in_hz, out_hz),
        (5, 1) => audio_rechannel_5_1(&input, in_hz, out_hz),
        (5, 2) => audio_rechannel_5_2(&input, in_hz, out_hz),
        (5, 3) => audio_rechannel_5_3(&input, in_hz, out_hz),
        (5, 4) => audio_rechannel_5_4(&input, in_hz, out_hz),
        (5, 6) => audio_rechannel_5_6(&input, in_hz, out_hz),
        (5, 7) => audio_rechannel_5_7(&input, in_hz, out_hz),
        (5, 8) => audio_rechannel_5_8(&input, in_hz, out_hz),
        (6, 1) => audio_rechannel_6_1(&input, in_hz, out_hz),
        (6, 2) => audio_rechannel_6_2(&input, in_hz, out_hz),
        (6, 3) => audio_rechannel_6_3(&input, in_hz, out_hz),
        (6, 4) => audio_rechannel_6_4(&input, in_hz, out_hz),
        (6, 5) => audio_rechannel_6_5(&input, in_hz, out_hz),
        (6, 7) => audio_rechannel_6_7(&input, in_hz, out_hz),
        (6, 8) => audio_rechannel_6_8(&input, in_hz, out_hz),
        (7, 1) => audio_rechannel_7_1(&input, in_hz, out_hz),
        (7, 2) => audio_rechannel_7_2(&input, in_hz, out_hz),
        (7, 3) => audio_rechannel_7_3(&input, in_hz, out_hz),
        (7, 4) => audio_rechannel_7_4(&input, in_hz, out_hz),
        (7, 5) => audio_rechannel_7_5(&input, in_hz, out_hz),
        (7, 6) => audio_rechannel_7_6(&input, in_hz, out_hz),
        (7, 8) => audio_rechannel_7_8(&input, in_hz, out_hz),
        (8, 1) => audio_rechannel_8_1(&input, in_hz, out_hz),
        (8, 2) => audio_rechannel_8_2(&input, in_hz, out_hz),
        (8, 3) => audio_rechannel_8_3(&input, in_hz, out_hz),
        (8, 4) => audio_rechannel_8_4(&input, in_hz, out_hz),
        (8, 5) => audio_rechannel_8_5(&input, in_hz, out_hz),
        (8, 6) => audio_rechannel_8_6(&input, in_hz, out_hz),
        (8, 7) => audio_rechannel_8_7(&input, in_hz, out_hz),
        _ => input,
    }
}

macro_rules! audio_rechannel {
    ($name:ident, $in_channels:expr, $out_channels:expr) => {
        fn $name(input: &[f32], in_hz: u32, out_hz: u32) -> Vec<f32> {
            use fon::{chan::Ch32, Audio, Frame};
            let mut in_audio =
                Audio::<Ch32, $in_channels>::with_silence(in_hz, input.len() / $in_channels);
            for (x, y) in input.chunks_exact($in_channels).zip(in_audio.iter_mut()) {
                let mut f = Frame::<Ch32, $in_channels>::default();
                let mut i = 0;
                for c in f.channels_mut() {
                    *c = x[i].into();
                    i += 1;
                }
                *y = f;
            }
            Audio::<Ch32, $out_channels>::with_audio(out_hz, &in_audio)
                .as_f32_slice()
                .to_owned()
        }
    };
}

audio_rechannel!(audio_rechannel_1_2, 1, 2);
audio_rechannel!(audio_rechannel_1_3, 1, 3);
audio_rechannel!(audio_rechannel_1_4, 1, 4);
audio_rechannel!(audio_rechannel_1_5, 1, 5);
audio_rechannel!(audio_rechannel_1_6, 1, 6);
audio_rechannel!(audio_rechannel_1_7, 1, 7);
audio_rechannel!(audio_rechannel_1_8, 1, 8);
audio_rechannel!(audio_rechannel_2_1, 2, 1);
audio_rechannel!(audio_rechannel_2_3, 2, 3);
audio_rechannel!(audio_rechannel_2_4, 2, 4);
audio_rechannel!(audio_rechannel_2_5, 2, 5);
audio_rechannel!(audio_rechannel_2_6, 2, 6);
audio_rechannel!(audio_rechannel_2_7, 2, 7);
audio_rechannel!(audio_rechannel_2_8, 2, 8);
audio_rechannel!(audio_rechannel_3_1, 3, 1);
audio_rechannel!(audio_rechannel_3_2, 3, 2);
audio_rechannel!(audio_rechannel_3_4, 3, 4);
audio_rechannel!(audio_rechannel_3_5, 3, 5);
audio_rechannel!(audio_rechannel_3_6, 3, 6);
audio_rechannel!(audio_rechannel_3_7, 3, 7);
audio_rechannel!(audio_rechannel_3_8, 3, 8);
audio_rechannel!(audio_rechannel_4_1, 4, 1);
audio_rechannel!(audio_rechannel_4_2, 4, 2);
audio_rechannel!(audio_rechannel_4_3, 4, 3);
audio_rechannel!(audio_rechannel_4_5, 4, 5);
audio_rechannel!(audio_rechannel_4_6, 4, 6);
audio_rechannel!(audio_rechannel_4_7, 4, 7);
audio_rechannel!(audio_rechannel_4_8, 4, 8);
audio_rechannel!(audio_rechannel_5_1, 5, 1);
audio_rechannel!(audio_rechannel_5_2, 5, 2);
audio_rechannel!(audio_rechannel_5_3, 5, 3);
audio_rechannel!(audio_rechannel_5_4, 5, 4);
audio_rechannel!(audio_rechannel_5_6, 5, 6);
audio_rechannel!(audio_rechannel_5_7, 5, 7);
audio_rechannel!(audio_rechannel_5_8, 5, 8);
audio_rechannel!(audio_rechannel_6_1, 6, 1);
audio_rechannel!(audio_rechannel_6_2, 6, 2);
audio_rechannel!(audio_rechannel_6_3, 6, 3);
audio_rechannel!(audio_rechannel_6_4, 6, 4);
audio_rechannel!(audio_rechannel_6_5, 6, 5);
audio_rechannel!(audio_rechannel_6_7, 6, 7);
audio_rechannel!(audio_rechannel_6_8, 6, 8);
audio_rechannel!(audio_rechannel_7_1, 7, 1);
audio_rechannel!(audio_rechannel_7_2, 7, 2);
audio_rechannel!(audio_rechannel_7_3, 7, 3);
audio_rechannel!(audio_rechannel_7_4, 7, 4);
audio_rechannel!(audio_rechannel_7_5, 7, 5);
audio_rechannel!(audio_rechannel_7_6, 7, 6);
audio_rechannel!(audio_rechannel_7_8, 7, 8);
audio_rechannel!(audio_rechannel_8_1, 8, 1);
audio_rechannel!(audio_rechannel_8_2, 8, 2);
audio_rechannel!(audio_rechannel_8_3, 8, 3);
audio_rechannel!(audio_rechannel_8_4, 8, 4);
audio_rechannel!(audio_rechannel_8_5, 8, 5);
audio_rechannel!(audio_rechannel_8_6, 8, 6);
audio_rechannel!(audio_rechannel_8_7, 8, 7);

pub fn run_me<T: AsRef<std::ffi::OsStr>>(args: Vec<T>) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "linux")]
    if let Ok(appdir) = std::env::var("APPDIR") {
        let appimage_cmd = std::path::Path::new(&appdir).join("AppRun");
        if appimage_cmd.exists() {
            log::info!("path: {:?}", appimage_cmd);
            return std::process::Command::new(appimage_cmd).args(&args).spawn();
        }
    }
    let cmd = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(cmd);
    #[cfg(windows)]
    let mut force_foreground = false;
    #[cfg(windows)]
    {
        let arg_strs = args
            .iter()
            .map(|x| x.as_ref().to_string_lossy())
            .collect::<Vec<_>>();
        if arg_strs == vec!["--install"] || arg_strs == &["--noinstall"] {
            cmd.env(crate::platform::SET_FOREGROUND_WINDOW, "1");
            force_foreground = true;
        }
    }
    let result = cmd.args(&args).spawn();
    match result.as_ref() {
        Ok(_child) =>
        {
            #[cfg(windows)]
            if force_foreground {
                unsafe { winapi::um::winuser::AllowSetForegroundWindow(_child.id() as u32) };
            }
        }
        Err(err) => log::error!("run_me: {err:?}"),
    }
    result
}

#[inline]
pub fn username() -> String {
    // fix bug of whoami
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return whoami::username().trim_end_matches('\0').to_owned();
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return DEVICE_NAME.lock().unwrap().clone();
}

// Exactly the implementation of "whoami::hostname()".
// This wrapper is to suppress warnings.
#[inline(always)]
#[cfg(not(target_os = "ios"))]
pub fn whoami_hostname() -> String {
    let mut hostname = whoami::fallible::hostname().unwrap_or_else(|_| "localhost".to_string());
    hostname.make_ascii_lowercase();
    hostname
}

#[inline]
pub fn hostname() -> String {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        #[allow(unused_mut)]
        let mut name = whoami_hostname();
        // some time, there is .local, some time not, so remove it for osx
        #[cfg(target_os = "macos")]
        if name.ends_with(".local") {
            name = name.trim_end_matches(".local").to_owned();
        }
        name
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    return DEVICE_NAME.lock().unwrap().clone();
}

#[inline]
pub fn get_sysinfo() -> serde_json::Value {
    use hbb_common::sysinfo::System;
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu();
    let memory = system.total_memory();
    let memory = (memory as f64 / 1024. / 1024. / 1024. * 100.).round() / 100.;
    let cpus = system.cpus();
    let cpu_name = cpus.first().map(|x| x.brand()).unwrap_or_default();
    let cpu_name = cpu_name.trim_end();
    let cpu_freq = cpus.first().map(|x| x.frequency()).unwrap_or_default();
    let cpu_freq = (cpu_freq as f64 / 1024. * 100.).round() / 100.;
    let cpu = if cpu_freq > 0. {
        format!("{}, {}GHz, ", cpu_name, cpu_freq)
    } else {
        "".to_owned() // android
    };
    let num_cpus = num_cpus::get();
    let num_pcpus = num_cpus::get_physical();
    let mut os = system.distribution_id();
    os = format!("{} / {}", os, system.long_os_version().unwrap_or_default());
    #[cfg(windows)]
    {
        os = format!("{os} - {}", system.os_version().unwrap_or_default());
    }
    let hostname = hostname(); // sys.hostname() return localhost on android in my test
    #[cfg(any(target_os = "android", target_os = "ios"))]
    let out;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let mut out;
    out = json!({
        "cpu": format!("{cpu}{num_cpus}/{num_pcpus} cores"),
        "memory": format!("{memory}GB"),
        "os": os,
        "hostname": hostname,
    });
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let username = crate::platform::get_active_username();
        if !username.is_empty() && (!cfg!(windows) || username != "SYSTEM") {
            out["username"] = json!(username);
        }
    }
    out
}

#[inline]
pub fn check_port<T: std::string::ToString>(host: T, port: i32) -> String {
    hbb_common::socket_client::check_port(host, port)
}

#[inline]
pub fn increase_port<T: std::string::ToString>(host: T, offset: i32) -> String {
    hbb_common::socket_client::increase_port(host, offset)
}

pub const POSTFIX_SERVICE: &'static str = "_service";

#[inline]
pub fn is_control_key(evt: &KeyEvent, key: &ControlKey) -> bool {
    if let Some(key_event::Union::ControlKey(ck)) = evt.union {
        ck.value() == key.value()
    } else {
        false
    }
}

#[inline]
pub fn is_modifier(evt: &KeyEvent) -> bool {
    if let Some(key_event::Union::ControlKey(ck)) = evt.union {
        let v = ck.value();
        v == ControlKey::Alt.value()
            || v == ControlKey::Shift.value()
            || v == ControlKey::Control.value()
            || v == ControlKey::Meta.value()
            || v == ControlKey::RAlt.value()
            || v == ControlKey::RShift.value()
            || v == ControlKey::RControl.value()
            || v == ControlKey::RWin.value()
    } else {
        false
    }
}

pub(crate) fn release_id_from_update_url(update_url: &str) -> ResultType<String> {
    let url = url::Url::parse(update_url)?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        bail!(
            "Update URL is not a GitHub HTTPS release URL: {}",
            update_url
        );
    }
    if url_has_explicit_port(update_url)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!(
            "Update URL must not contain credentials or an explicit port: {}",
            update_url
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!(
            "Update URL must not contain query or fragment: {}",
            update_url
        );
    }

    let Some(segments) = url.path_segments() else {
        bail!("Update URL has no path: {}", update_url);
    };
    let segments = segments.collect::<Vec<_>>();
    let release_id = match segments.as_slice() {
        ["rustdesk", "rustdesk", "releases", "tag", release_id] => *release_id,
        ["rustdesk", "rustdesk", "releases", "tag", release_id, ""] => *release_id,
        _ => bail!(
            "Update URL is not a RustDesk release tag URL: {}",
            update_url
        ),
    };
    if release_id.is_empty()
        || release_id == "."
        || release_id == ".."
        || release_id.contains([' ', '/', '\\', '?', '#'])
        || release_id.contains('%')
    {
        bail!("Update URL has no release id: {}", update_url);
    }
    Ok(release_id.to_owned())
}

pub(crate) fn url_has_explicit_port(raw_url: &str) -> bool {
    let Some((_, rest)) = raw_url.split_once("://") else {
        return false;
    };
    let authority = rest
        .split(|c| matches!(c, '/' | '?' | '#'))
        .next()
        .unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    host_port.rsplit_once(':').is_some()
}

pub(crate) fn display_version_from_release_id(release_id: &str) -> ResultType<String> {
    let display_version = release_id.strip_prefix('v').unwrap_or(release_id);
    let segments = display_version.split('.').collect::<Vec<_>>();
    if segments.len() != 3
        || segments.iter().any(|segment| {
            segment.is_empty()
                || !segment.chars().all(|c| c.is_ascii_digit())
                || segment.parse::<u16>().is_err()
        })
    {
        bail!("Release id is not a stable display version: {}", release_id);
    }
    if get_version_number(display_version) <= 0 {
        bail!("Release id has invalid display version: {}", release_id);
    }
    Ok(display_version.to_owned())
}

#[inline]
pub fn get_app_name() -> String {
    hbb_common::config::APP_NAME.read().unwrap().clone()
}

#[inline]
pub fn is_rustdesk() -> bool {
    hbb_common::config::APP_NAME.read().unwrap().eq("RustDesk")
}

#[inline]
pub fn is_custom_client() -> bool {
    // The old check compared with "RustDesk", which is true for every SealDesk build
    // and would send the Windows installer down the pro custom-client path.
    get_app_name() != "SealDesk"
}

#[inline]
pub fn get_uri_prefix() -> String {
    "sealdesk://".to_owned()
}

#[cfg(target_os = "macos")]
pub fn get_full_name() -> String {
    format!(
        "{}.{}",
        hbb_common::config::ORG.read().unwrap(),
        hbb_common::config::APP_NAME.read().unwrap(),
    )
}

pub fn is_setup(name: &str) -> bool {
    !config::is_disable_installation() && name.to_lowercase().ends_with("install.exe")
}

pub fn get_local_option(key: &str) -> String {
    let v = LocalConfig::get_option(key);
    if key == "lang" && (v == "pt" || v == "br") {
        return "pt-br".to_owned();
    }
    v
}

#[inline]
pub fn make_privacy_mode_msg_with_details(
    state: back_notification::PrivacyModeState,
    details: String,
    impl_key: String,
) -> Message {
    let mut misc = Misc::new();
    let mut back_notification = BackNotification {
        details,
        impl_key,
        ..Default::default()
    };
    back_notification.set_privacy_mode_state(state);
    misc.set_back_notification(back_notification);
    let mut msg_out = Message::new();
    msg_out.set_misc(misc);
    msg_out
}

#[inline]
pub fn make_privacy_mode_msg(
    state: back_notification::PrivacyModeState,
    impl_key: String,
) -> Message {
    make_privacy_mode_msg_with_details(state, "".to_owned(), impl_key)
}

pub fn is_keyboard_mode_supported(
    keyboard_mode: &KeyboardMode,
    version_number: i64,
    peer_platform: &str,
) -> bool {
    match keyboard_mode {
        KeyboardMode::Legacy => true,
        KeyboardMode::Map => {
            if peer_platform.to_lowercase() == crate::PLATFORM_ANDROID.to_lowercase() {
                false
            } else {
                version_number >= hbb_common::get_version_number("1.2.0")
            }
        }
        KeyboardMode::Translate => version_number >= hbb_common::get_version_number("1.2.0"),
        KeyboardMode::Auto => version_number >= hbb_common::get_version_number("1.2.0"),
    }
}

pub fn get_supported_keyboard_modes(version: i64, peer_platform: &str) -> Vec<KeyboardMode> {
    KeyboardMode::iter()
        .filter(|&mode| is_keyboard_mode_supported(mode, version, peer_platform))
        .map(|&mode| mode)
        .collect::<Vec<_>>()
}

pub fn make_fd_to_json(id: i32, path: String, entries: &Vec<FileEntry>) -> String {
    let fd_json = _make_fd_to_json(id, path, entries);
    serde_json::to_string(&fd_json).unwrap_or("".into())
}

pub fn _make_fd_to_json(id: i32, path: String, entries: &Vec<FileEntry>) -> Map<String, Value> {
    let mut fd_json = serde_json::Map::new();
    fd_json.insert("id".into(), json!(id));
    fd_json.insert("path".into(), json!(path));

    let mut entries_out = vec![];
    for entry in entries {
        let mut entry_map = serde_json::Map::new();
        entry_map.insert("entry_type".into(), json!(entry.entry_type.value()));
        entry_map.insert("name".into(), json!(entry.name));
        entry_map.insert("size".into(), json!(entry.size));
        entry_map.insert("modified_time".into(), json!(entry.modified_time));
        entries_out.push(entry_map);
    }
    fd_json.insert("entries".into(), json!(entries_out));
    fd_json
}

pub fn make_vec_fd_to_json(fds: &[FileDirectory]) -> String {
    let mut fd_jsons = vec![];

    for fd in fds.iter() {
        let fd_json = _make_fd_to_json(fd.id, fd.path.clone(), &fd.entries);
        fd_jsons.push(fd_json);
    }

    serde_json::to_string(&fd_jsons).unwrap_or("".into())
}

pub fn make_empty_dirs_response_to_json(res: &ReadEmptyDirsResponse) -> String {
    let mut map: Map<String, Value> = serde_json::Map::new();
    map.insert("path".into(), json!(res.path));

    let mut fd_jsons = vec![];

    for fd in res.empty_dirs.iter() {
        let fd_json = _make_fd_to_json(fd.id, fd.path.clone(), &fd.entries);
        fd_jsons.push(fd_json);
    }
    map.insert("empty_dirs".into(), fd_jsons.into());

    serde_json::to_string(&map).unwrap_or("".into())
}

/// The function to handle the url scheme sent by the system.
///
/// 1. Try to send the url scheme from ipc.
/// 2. If failed to send the url scheme, we open a new main window to handle this url scheme.
pub fn handle_url_scheme(url: String) {
    #[cfg(not(target_os = "ios"))]
    if let Err(err) = crate::ipc::send_url_scheme(url.clone()) {
        log::debug!("Send the url to the existing flutter process failed, {}. Let's open a new program to handle this.", err);
        let _ = crate::run_me(vec![url]);
    }
}

#[inline]
pub fn encode64<T: AsRef<[u8]>>(input: T) -> String {
    #[allow(deprecated)]
    base64::encode(input)
}

#[inline]
pub fn decode64<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
    #[allow(deprecated)]
    base64::decode(input)
}

pub fn pk_to_fingerprint(pk: Vec<u8>) -> String {
    let s: String = pk.iter().map(|u| format!("{:02x}", u)).collect();
    s.chars()
        .enumerate()
        .map(|(i, c)| {
            if i > 0 && i % 4 == 0 {
                format!(" {}", c)
            } else {
                format!("{}", c)
            }
        })
        .collect()
}

#[cfg(all(target_os = "windows", not(target_pointer_width = "64")))]
pub fn check_process(arg: &str, same_session_id: bool) -> bool {
    let mut path = std::env::current_exe().unwrap_or_default();
    if let Ok(linked) = path.read_link() {
        path = linked;
    }
    let Some(filename) = path.file_name() else {
        return false;
    };
    let filename = filename.to_string_lossy().to_string();
    match crate::platform::windows::get_pids_with_first_arg_check_session(
        &filename,
        arg,
        same_session_id,
    ) {
        Ok(pids) => {
            let self_pid = hbb_common::sysinfo::Pid::from_u32(std::process::id());
            pids.into_iter().filter(|pid| *pid != self_pid).count() > 0
        }
        Err(e) => {
            log::error!("Failed to check process with arg: \"{}\", {}", arg, e);
            false
        }
    }
}

#[allow(unused_mut)]
#[cfg(not(all(target_os = "windows", not(target_pointer_width = "64"))))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn check_process(arg: &str, mut same_uid: bool) -> bool {
    #[cfg(target_os = "macos")]
    if !crate::platform::is_root() && !same_uid {
        log::warn!("Can not get other process's command line arguments on macos without root");
        same_uid = true;
    }
    use hbb_common::sysinfo::System;
    let mut sys = System::new();
    sys.refresh_processes();
    let mut path = std::env::current_exe().unwrap_or_default();
    if let Ok(linked) = path.read_link() {
        path = linked;
    }
    let path = path.to_string_lossy().to_lowercase();
    let my_uid = sys
        .process((std::process::id() as usize).into())
        .map(|x| x.user_id())
        .unwrap_or_default();
    for (_, p) in sys.processes().iter() {
        let mut cur_path = p.exe().to_path_buf();
        if let Ok(linked) = cur_path.read_link() {
            cur_path = linked;
        }
        if cur_path.to_string_lossy().to_lowercase() != path {
            continue;
        }
        if p.pid().to_string() == std::process::id().to_string() {
            continue;
        }
        if same_uid && p.user_id() != my_uid {
            continue;
        }
        // on mac, p.cmd() get "/Applications/RustDesk.app/Contents/MacOS/RustDesk", "XPC_SERVICE_NAME=com.carriez.RustDesk_server"
        let parg = if p.cmd().len() <= 1 { "" } else { &p.cmd()[1] };
        if arg.is_empty() {
            if !parg.starts_with("--") {
                return true;
            }
        } else if arg == parg {
            return true;
        }
    }
    false
}

pub struct ThrottledInterval {
    interval: Interval,
    next_tick: Instant,
    min_interval: Duration,
}

impl ThrottledInterval {
    pub fn new(i: Interval) -> ThrottledInterval {
        let period = i.period();
        ThrottledInterval {
            interval: i,
            next_tick: Instant::now(),
            min_interval: Duration::from_secs_f64(period.as_secs_f64() * 0.9),
        }
    }

    pub async fn tick(&mut self) -> Instant {
        let instant = poll_fn(|cx| self.poll_tick(cx));
        instant.await
    }

    pub fn poll_tick(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Instant> {
        match self.interval.poll_tick(cx) {
            Poll::Ready(instant) => {
                let now = Instant::now();
                if self.next_tick <= now {
                    self.next_tick = now + self.min_interval;
                    Poll::Ready(instant)
                } else {
                    // This call is required since tokio 1.27
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub type RustDeskInterval = ThrottledInterval;

#[inline]
pub fn rustdesk_interval(i: Interval) -> ThrottledInterval {
    ThrottledInterval::new(i)
}

#[inline]
#[cfg(target_os = "macos")]
pub fn get_dst_align_rgba() -> usize {
    // https://developer.apple.com/forums/thread/712709
    // Memory alignment should be multiple of 64.
    if crate::ui_interface::use_texture_render() {
        64
    } else {
        1
    }
}

#[inline]
#[cfg(not(target_os = "macos"))]
pub fn get_dst_align_rgba() -> usize {
    1
}

#[inline]
pub fn is_empty_uni_link(arg: &str) -> bool {
    let prefix = crate::get_uri_prefix();
    if !arg.starts_with(&prefix) {
        return false;
    }
    arg[prefix.len()..].chars().all(|c| c == '/')
}

pub fn get_hwid() -> Bytes {
    use hbb_common::sha2::{Digest, Sha256};

    let uuid = hbb_common::get_uuid();
    let mut hasher = Sha256::new();
    hasher.update(&uuid);
    Bytes::from(hasher.finalize().to_vec())
}

#[inline]
pub fn get_builtin_option(key: &str) -> String {
    config::BUILTIN_SETTINGS
        .read()
        .unwrap()
        .get(key)
        .cloned()
        .unwrap_or_default()
}

pub fn verify_login(_raw: &str, _id: &str) -> bool {
    true
    /*
    if is_custom_client() {
        return true;
    }
    #[cfg(debug_assertions)]
    return true;
    let Ok(pk) = crate::decode64("IycjQd4TmWvjjLnYd796Rd+XkK+KG+7GU1Ia7u4+vSw=") else {
        return false;
    };
    let Some(key) = get_pk(&pk).map(|x| sign::PublicKey(x)) else {
        return false;
    };
    let Ok(v) = crate::decode64(raw) else {
        return false;
    };
    let raw = sign::verify(&v, &key).unwrap_or_default();
    let v_str = std::str::from_utf8(&raw)
        .unwrap_or_default()
        .split(":")
        .next()
        .unwrap_or_default();
    v_str == id
    */
}

// The color is the same to `str2color()` in flutter.
pub fn str2color(s: &str, alpha: u8) -> u32 {
    let bytes = s.as_bytes();
    // dart code `160 << 16 + 114 << 8 + 91` results `0`.
    let mut hash: u32 = 0;
    for &byte in bytes {
        let code = byte as u32;
        hash = code.wrapping_add((hash << 5).wrapping_sub(hash));
    }

    hash = hash % 16777216;
    let rgb = hash & 0xFF7FFF;

    (alpha as u32) << 24 | rgb
}

/// Check control permission state from a u64 bitmap.
/// Each permission uses 2 bits: 0 = not set, 1 = disable, 2 = enable, 3 = invalid (treated as not set)
/// Returns: Some(true) = enabled, Some(false) = disabled, None = not set or invalid
pub fn get_control_permission(
    permissions: u64,
    permission: hbb_common::rendezvous_proto::control_permissions::Permission,
) -> Option<bool> {
    use hbb_common::protobuf::Enum;
    let index = permission.value();
    if index >= 0 && index < 32 {
        let shift = index * 2;
        let value = (permissions >> shift) & 0b11;
        match value {
            1 => Some(false), // disable
            2 => Some(true),  // enable
            _ => None,        // 0 = not set, 3 = invalid
        }
    } else {
        None
    }
}

// Align the maximum length of the peer id to the maximum length of the peer id in the server.
const MAX_UNTRUSTED_PEER_ID_LEN: usize = 253;
const UNTRUSTED_PEER_ID_FORBIDDEN_CHARS: &[char] = &['"', '<', '>', '/', '\\', '|', '?', '*'];

// Shared validation for peer/connect ids that cross untrusted boundaries before
// they are stored or written into command/script contexts.
pub fn is_valid_untrusted_peer_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_UNTRUSTED_PEER_ID_LEN
        && !id.chars().any(|ch| {
            ch.is_control() || ch.is_whitespace() || UNTRUSTED_PEER_ID_FORBIDDEN_CHARS.contains(&ch)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hbb_common::tokio::{
        self,
        time::{interval, interval_at, sleep, Duration, Instant, Interval},
    };
    use std::collections::HashSet;

    #[test]
    fn parses_stable_rustdesk_release_url() {
        let update_url = "https://github.com/rustdesk/rustdesk/releases/tag/v1.4.6";

        assert_eq!(release_id_from_update_url(update_url).unwrap(), "v1.4.6");
        assert_eq!(display_version_from_release_id("v1.4.6").unwrap(), "1.4.6");
        assert_eq!(
            release_id_from_update_url(&format!("{update_url}/")).unwrap(),
            "v1.4.6"
        );
    }

    #[test]
    fn rejects_untrusted_release_urls_and_unstable_versions() {
        for update_url in [
            "http://github.com/rustdesk/rustdesk/releases/tag/v1.4.6",
            "https://github.com:8443/rustdesk/rustdesk/releases/tag/v1.4.6",
            "https://github.com:443/rustdesk/rustdesk/releases/tag/v1.4.6",
            "https://user@github.com/rustdesk/rustdesk/releases/tag/v1.4.6",
            "https://example.com/rustdesk/rustdesk/releases/tag/v1.4.6",
            "https://github.com/other/rustdesk/releases/tag/v1.4.6",
            "https://github.com/rustdesk/other/releases/tag/v1.4.6",
            "https://github.com/rustdesk/rustdesk/releases/download/v1.4.6/rustdesk.exe",
            "https://github.com/rustdesk/rustdesk/releases/tag/v1.4.6?x=1",
            "https://github.com/rustdesk/rustdesk/releases/tag/v1.4.6#asset",
            "https://github.com/rustdesk/rustdesk/releases/tag/v1.4.6/extra",
            "https://github.com/rustdesk/rustdesk/releases/tag/v1%2F4%2F6",
            "https://github.com/rustdesk/rustdesk/releases/tag/%2e%2e",
        ] {
            assert!(release_id_from_update_url(update_url).is_err());
        }
        for release_id in ["v1.4.6-rc1", "nightly", "v9223372036854775807.1.1"] {
            assert!(display_version_from_release_id(release_id).is_err());
        }
    }

    #[test]
    fn a_cursor_content_id_fits_a_web_client_number() {
        const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;
        for colors in [&b"arrow"[..], b"beam", b"hand", b""] {
            for hotx in 0..64 {
                assert!(cursor_content_id(32, 32, hotx, 0, colors) <= MAX_SAFE_INTEGER);
            }
        }
    }

    #[inline]
    fn get_timestamp_secs() -> u128 {
        (std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .unwrap()
            .as_millis()
            + 500)
            / 1000
    }

    fn interval_maker() -> Interval {
        interval(Duration::from_secs(1))
    }

    fn interval_at_maker() -> Interval {
        interval_at(
            Instant::now() + Duration::from_secs(1),
            Duration::from_secs(1),
        )
    }

    // The deadline must hold against a peer that keeps the receive side ready. `select!` rebuilds
    // its arms every iteration, so a relative sleep would be restarted by every datagram and the
    // punch would run for as long as the peer keeps talking, with no outer timeout to stop it.
    #[cfg(feature = "flutter")]
    #[tokio::test]
    async fn test_udp_punch_deadline_survives_a_talkative_peer() {
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (a_addr, b_addr) = (a.local_addr().unwrap(), b.local_addr().unwrap());
        a.connect(b_addr).await.unwrap();
        b.connect(a_addr).await.unwrap();
        // Empty datagrams answer no probe and match no return branch, so they only feed the loop.
        // Sent well past the punch deadline so a restarted timer would show up as a long run.
        let flooder = tokio::spawn(async move {
            let end = Instant::now() + Duration::from_secs(12);
            while Instant::now() < end {
                if b.send(&[]).await.is_err() {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        });
        let start = Instant::now();
        let res = punch_udp(Arc::new(a), false).await;
        let elapsed = start.elapsed();
        flooder.abort();
        assert!(res.is_err(), "the punch should have timed out");
        assert!(
            elapsed < Duration::from_secs(6),
            "the punch ran for {elapsed:?}; its deadline did not hold"
        );
    }

    #[test]
    fn untrusted_peer_id_validation() {
        let cases = [
            ("123456789", true),
            ("m\u{00FC}nchen-pc", true),
            ("192.168.1.10:21118", true),
            ("9123456234@public", true),
            (
                r#"1" & oWS.Run("cmd.exe /k whoami /priv",1,False) & ""#,
                false,
            ),
            ("", false),
            ("peer id", false),
            ("peer\nid", false),
            ("peer/id", false),
            ("peer?id", false),
        ];

        for (id, expected) in cases {
            assert_eq!(is_valid_untrusted_peer_id(id), expected, "{id:?}");
        }
    }

    // ThrottledInterval tick less times than tokio interval, if there're sleeps
    #[allow(non_snake_case)]
    #[tokio::test]
    async fn test_RustDesk_interval_sleep() {
        let base_intervals = [interval_maker, interval_at_maker];
        for (i, maker) in base_intervals.into_iter().enumerate() {
            let mut timer = rustdesk_interval(maker());
            let mut times = Vec::new();
            sleep(Duration::from_secs(3)).await;
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        times.push(get_timestamp_secs());
                        if times.len() == 5 {
                            break;
                        }
                    }
                }
            }
            // No multiple ticks in the `interval` time.
            // Values in "times" are unique and are less than normal tokio interval.
            let times2: HashSet<u128> = HashSet::from_iter(times.clone());
            assert_eq!(times.len(), times2.len(), "test: {}", i);
        }
    }

    #[cfg(feature = "flutter")]
    #[test]
    fn test_is_public_matches_rustdesk_root_domain() {
        assert!(is_public("rustdesk.com/"));
        assert!(is_public("rustdesk.com:21117"));
        assert!(is_public("api.rustdesk.com:21117"));
        assert!(!is_public("hello-rustdesk.com"));
        assert!(!is_public("api.rustdesk.com.evil.test"));
        assert!(!is_public("https://rustdesk.com@evil.test"));
    }

    #[cfg(feature = "flutter")]
    #[test]
    fn test_should_use_tcp_proxy_for_api_url() {
        assert!(should_use_tcp_proxy_for_api_url(
            "https://admin.example.com/api/login",
            "https://admin.example.com"
        ));
        assert!(should_use_tcp_proxy_for_api_url(
            "https://admin.example.com:21114/api/login",
            "https://admin.example.com"
        ));
        assert!(!should_use_tcp_proxy_for_api_url(
            "https://api.telegram.org/bot123/sendMessage",
            "https://admin.example.com"
        ));
        assert!(!should_use_tcp_proxy_for_api_url(
            "https://admin.rustdesk.com/api/login",
            "https://admin.rustdesk.com"
        ));
        assert!(!should_use_tcp_proxy_for_api_url(
            "https://admin.example.com/api/login",
            "not a url"
        ));
        assert!(!should_use_tcp_proxy_for_api_url(
            "not a url",
            "https://admin.example.com"
        ));
    }

    #[cfg(feature = "flutter")]
    #[test]
    fn test_parse_simple_header_preserves_non_content_type_header() {
        let headers = parse_simple_header("Authorization: Bearer token");

        assert!(headers.iter().any(|entry| {
            entry.name.eq_ignore_ascii_case("Authorization")
                && entry.value.as_str() == "Bearer token"
        }));
        assert_eq!(
            headers
                .iter()
                .filter(|entry| entry.name.eq_ignore_ascii_case("Content-Type"))
                .count(),
            1
        );
        assert_eq!(
            headers
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case("Content-Type"))
                .map(|entry| entry.value.as_str()),
            Some("application/json")
        );
    }

    #[cfg(feature = "flutter")]
    #[test]
    fn test_http_proxy_response_to_json() {
        let mut resp = HttpProxyResponse {
            status: 200,
            body: br#"{"ok":true}"#.to_vec().into(),
            ..Default::default()
        };
        resp.headers.push(HeaderEntry {
            name: "Content-Type".into(),
            value: "application/json".into(),
            ..Default::default()
        });

        let json = http_proxy_response_to_json(resp).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["status_code"], 200);
        assert_eq!(value["headers"]["content-type"], "application/json");
        assert_eq!(value["body"], r#"{"ok":true}"#);

        let err = http_proxy_response_to_json(HttpProxyResponse {
            error: "dial failed".into(),
            ..Default::default()
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("TCP proxy error: dial failed"));
    }

    /// A stand-in rendezvous server on loopback: accepts one connection and hands it to `serve`.
    #[cfg(feature = "flutter")]
    async fn rendezvous_stub<F, Fut>(serve: F) -> String
    where
        F: FnOnce(hbb_common::tcp::FramedStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = hbb_common::tcp::new_listener("127.0.0.1:0", false)
            .await
            .unwrap();
        let host = listener.local_addr().unwrap().to_string();
        tokio::spawn(async move {
            if let Ok((stream, addr)) = listener.accept().await {
                serve(hbb_common::tcp::FramedStream::from(stream, addr)).await;
            }
        });
        host
    }

    #[cfg(feature = "flutter")]
    fn server_key() -> (String, sign::SecretKey) {
        let (pk, sk) = sign::gen_keypair();
        (encode64(pk.0), sk)
    }

    #[cfg(feature = "flutter")]
    fn signed_key_exchange(
        pk: &box_::PublicKey,
        sk: &sign::SecretKey,
        version: u32,
    ) -> KeyExchange {
        let params = KxParams {
            pk: pk.0.to_vec().into(),
            version,
            ..Default::default()
        };
        let mut payload = hbb_common::tcp::KX_PARAMS_DOMAIN.to_vec();
        payload.extend_from_slice(&params.write_to_bytes().unwrap());
        KeyExchange {
            keys: vec![sign::sign(&pk.0, sk).into()],
            version,
            signed_params: sign::sign(&payload, sk).into(),
            ..Default::default()
        }
    }

    #[cfg(feature = "flutter")]
    async fn connect(host: &str) -> Stream {
        hbb_common::socket_client::connect_tcp(host.to_owned(), 3000)
            .await
            .unwrap()
    }

    #[cfg(feature = "flutter")]
    #[tokio::test]
    async fn test_secure_tcp_required_refuses_a_server_without_the_exchange() {
        let (key, _) = server_key();
        // A server from before the exchange answers the first message with something else.
        let serve = |mut s: hbb_common::tcp::FramedStream| async move {
            let mut msg = RendezvousMessage::new();
            msg.set_register_peer_response(RegisterPeerResponse::new());
            s.send(&msg).await.unwrap();
            sleep(Duration::from_secs(2)).await;
        };
        let host = rendezvous_stub(serve).await;
        let mut conn = connect(&host).await;
        assert!(secure_tcp_required(&mut conn, &key).await.is_err());
        assert!(!conn.is_secured());
        // The legacy call tolerates the same server, and the stream stays in the clear.
        let host = rendezvous_stub(serve).await;
        let mut conn = connect(&host).await;
        secure_tcp(&mut conn, &key).await.unwrap();
        assert!(!conn.is_secured());
    }

    #[cfg(feature = "flutter")]
    #[tokio::test]
    async fn test_secure_tcp_legacy_and_signed_servers_exchange_application_data() {
        for (advertised, signed) in [(0, false), (1, true), (3, true)] {
            for required in [false, true] {
                let (key, sk) = server_key();
                let host = rendezvous_stub(move |mut s| async move {
                    let (mut eph_pk, eph_sk) = box_::gen_keypair();
                    let mut msg = RendezvousMessage::new();
                    let ex = if signed {
                        eph_pk.0[31] |= 0x80;
                        signed_key_exchange(&eph_pk, &sk, advertised)
                    } else {
                        KeyExchange {
                            keys: vec![sign::sign(&eph_pk.0, &sk).into()],
                            ..Default::default()
                        }
                    };
                    msg.set_key_exchange(ex);
                    s.send(&msg).await.unwrap();
                    let reply = s.next_timeout(3000).await.unwrap().unwrap();
                    let reply = RendezvousMessage::parse_from_bytes(&reply).unwrap();
                    let Some(rendezvous_message::Union::KeyExchange(ex)) = reply.union else {
                        panic!("expected the client's key exchange");
                    };
                    assert_eq!(ex.keys.len(), 2);
                    assert_eq!(ex.version, hbb_common::tcp::kx_version_for(advertised));
                    let shared =
                        hbb_common::tcp::Encrypt::decode(&ex.keys[1], &ex.keys[0], &eph_sk)
                            .unwrap();
                    if ex.version >= 1 {
                        s.set_key_split(
                            shared,
                            false,
                            &hbb_common::tcp::KxTranscript {
                                initiator_pk: &ex.keys[0],
                                responder_pk: &eph_pk.0,
                                advertised,
                                picked: ex.version,
                            },
                        )
                        .unwrap();
                    } else {
                        s.set_key(shared);
                    }
                    let request = s.next_timeout(3000).await.unwrap().unwrap();
                    let request = RendezvousMessage::parse_from_bytes(&request).unwrap();
                    let Some(rendezvous_message::Union::TestNatRequest(nat)) = request.union else {
                        panic!("expected the client's nat request");
                    };
                    let mut msg = RendezvousMessage::new();
                    msg.set_test_nat_response(TestNatResponse {
                        port: nat.serial,
                        ..Default::default()
                    });
                    s.send(&msg).await.unwrap();
                })
                .await;
                let mut conn = connect(&host).await;
                if required {
                    secure_tcp_required(&mut conn, &key).await.unwrap();
                } else {
                    secure_tcp(&mut conn, &key).await.unwrap();
                }
                assert!(conn.is_secured());
                let mut msg = RendezvousMessage::new();
                msg.set_test_nat_request(TestNatRequest {
                    serial: 7,
                    ..Default::default()
                });
                conn.send(&msg).await.unwrap();
                let reply = conn.next_timeout(3000).await.unwrap().unwrap();
                let reply = RendezvousMessage::parse_from_bytes(&reply).unwrap();
                let Some(rendezvous_message::Union::TestNatResponse(nat)) = reply.union else {
                    panic!("expected the server's nat response");
                };
                assert_eq!(nat.port, 7);
            }
        }
    }

    #[cfg(feature = "flutter")]
    #[tokio::test]
    async fn test_secure_tcp_rejects_unverified_versions_before_reply() {
        let (key, sk) = server_key();
        let (mut eph_pk, _) = box_::gen_keypair();
        eph_pk.0[31] |= 0x80;
        let (other_pk, _) = box_::gen_keypair();
        let (_, other_sk) = sign::gen_keypair();
        for case in [
            "missing_signature",
            "stripped_version_and_signature",
            "cleared_marker_and_stripped_fields",
            "lowered_version",
            "raised_version",
            "replaced_public_key",
            "invalid_signature",
            "wrong_signer",
            "unstructured_payload",
            "undomained_params",
            "signed_id_pk_as_params",
        ] {
            let mut ex = signed_key_exchange(&eph_pk, &sk, 1);
            match case {
                "missing_signature" => ex.signed_params = Bytes::new(),
                "stripped_version_and_signature" => {
                    ex.signed_params = Bytes::new();
                    ex.version = 0;
                }
                "cleared_marker_and_stripped_fields" => {
                    let mut signed_pk = ex.keys[0].to_vec();
                    *signed_pk.last_mut().unwrap() &= 0x7f;
                    ex.keys[0] = signed_pk.into();
                    ex.signed_params = Bytes::new();
                    ex.version = 0;
                }
                "lowered_version" => ex.version = 0,
                "raised_version" => ex.version = 2,
                "replaced_public_key" => ex.keys[0] = sign::sign(&other_pk.0, &sk).into(),
                "invalid_signature" => {
                    let mut signed = ex.signed_params.to_vec();
                    signed[0] ^= 1;
                    ex.signed_params = signed.into();
                }
                "wrong_signer" => {
                    ex.signed_params = signed_key_exchange(&eph_pk, &other_sk, 1).signed_params;
                }
                "unstructured_payload" => {
                    let mut payload = eph_pk.0.to_vec();
                    payload.extend_from_slice(&1u32.to_le_bytes());
                    ex.signed_params = sign::sign(&payload, &sk).into();
                }
                "undomained_params" => {
                    let params = KxParams {
                        pk: eph_pk.0.to_vec().into(),
                        version: 1,
                        ..Default::default()
                    };
                    ex.signed_params = sign::sign(&params.write_to_bytes().unwrap(), &sk).into();
                }
                "signed_id_pk_as_params" => {
                    // The server signs IdPk with the same key; its `id` sits where `pk` does.
                    let mut pk = [0x2au8; box_::PUBLICKEYBYTES];
                    pk[30] = 0xc2;
                    pk[31] = 0xaa;
                    let id_pk = IdPk {
                        id: String::from_utf8(pk.to_vec()).unwrap(),
                        pk: vec![7u8; box_::PUBLICKEYBYTES].into(),
                        ..Default::default()
                    };
                    ex.keys[0] = sign::sign(&pk, &sk).into();
                    ex.version = 0;
                    ex.signed_params = sign::sign(&id_pk.write_to_bytes().unwrap(), &sk).into();
                }
                _ => unreachable!(),
            }
            for required in [false, true] {
                let mut msg = RendezvousMessage::new();
                msg.set_key_exchange(ex.clone());
                let (tx, rx) = tokio::sync::oneshot::channel();
                let host = rendezvous_stub(move |mut s| async move {
                    s.send(&msg).await.unwrap();
                    tx.send(s.next_timeout(3000).await.is_none()).unwrap();
                })
                .await;
                let mut conn = connect(&host).await;
                let result = if required {
                    secure_tcp_required(&mut conn, &key).await
                } else {
                    secure_tcp(&mut conn, &key).await
                };
                assert!(result.is_err(), "accepted {case}, required={required}");
                assert!(!conn.is_secured());
                drop(conn);
                assert!(rx.await.unwrap(), "replied to {case}, required={required}");
            }
        }
    }

    #[cfg(feature = "flutter")]
    #[test]
    fn test_dtls_fingerprint_travels_signed_and_binds() {
        let (pk, sk) = sign::gen_keypair();
        let fp = "sha-256 0A:1B:2C";
        let signed = sign::sign(
            &IdPk {
                id: "123456789".to_owned(),
                pk: Bytes::from(vec![7u8; 32]),
                dtls_fingerprint: fp.to_owned(),
                ..Default::default()
            }
            .write_to_bytes()
            .unwrap(),
            &sk,
        );

        let (id, their_pk, signed_fp, _) = decode_id_pk_dtls(&signed, &pk).unwrap();
        assert_eq!(id, "123456789");
        assert_eq!(their_pk, [7u8; 32]);
        assert_eq!(signed_fp, fp);
        assert!(dtls_fingerprint_bound(&signed_fp, fp));
        assert!(!dtls_fingerprint_bound(&signed_fp, "sha-256 0A:1B:2D"));
        assert!(!dtls_fingerprint_bound("", ""));

        // The fingerprint is under the signature: a blob verified with another key yields
        // nothing, and one whose payload was edited in transit fails verification.
        let (other_pk, _) = sign::gen_keypair();
        assert!(decode_id_pk_dtls(&signed, &other_pk).is_err());
        let mut tampered = signed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(decode_id_pk_dtls(&tampered, &pk).is_err());

        // `decode_id_pk` is the same blob minus the fingerprint, so the field is invisible to
        // non-WebRTC handshakes.
        assert_eq!(decode_id_pk(&signed, &pk).unwrap(), (id, their_pk));
    }
}
