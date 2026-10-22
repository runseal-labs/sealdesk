#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::clipboard::clipboard_listener::{self, ClipboardEvent};
use async_trait::async_trait;
use bytes::Bytes;
#[cfg(not(target_os = "linux"))]
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Host, StreamConfig,
};
use crossbeam_queue::ArrayQueue;
use magnum_opus::{Channels::*, Decoder as AudioDecoder};
#[cfg(not(target_os = "linux"))]
use ringbuf::{ring_buffer::RbBase, Rb};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    ops::Deref,
    str::FromStr,
    sync::{
        mpsc::{self, RecvTimeoutError},
        Arc, Mutex, RwLock,
    },
};
use uuid::Uuid;

use crate::{
    common::input::{MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT, MOUSE_TYPE_DOWN, MOUSE_TYPE_UP},
    is_keyboard_mode_supported,
    ui_interface::{get_builtin_option, resolve_avatar_url, use_texture_render},
    ui_session_interface::{InvokeUiSession, Session},
};
#[cfg(feature = "unix-file-copy-paste")]
use crate::{clipboard::check_clipboard_files, clipboard_file::unix_file_clip};
pub use file_trait::FileManager;
#[cfg(not(feature = "flutter"))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use hbb_common::tokio::sync::mpsc::UnboundedSender;
use hbb_common::{
    allow_err,
    anyhow::{anyhow, Context},
    bail,
    config::{
        self, Config, LocalConfig, PeerConfig, PeerInfoSerde, Resolution,
        CONNECT_TIMEOUT,
    },
    futures::future::FutureExt,
    get_version_number, log,
    protobuf::{Message as _, MessageField},
    rendezvous_proto::*,
    rand,
    sha2::{Digest, Sha256},
    sodiumoxide::base64,
    tokio::{
        self,
        sync::mpsc::{unbounded_channel, UnboundedReceiver},
        time::{interval, Duration, Instant},
    }, ResultType, Stream,
};
use base::{
    config::keys,
    fs::JobType,
    message_proto::{option_message::BoolOption, *},
};
pub use helper::*;
use scrap::{
    codec::Decoder,
    record::{Recorder, RecorderContext},
    CodecFormat, ImageFormat, ImageRgb, ImageTexture,
};

#[cfg(not(target_os = "ios"))]
use crate::clipboard::CLIPBOARD_INTERVAL;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::clipboard::{check_clipboard, ClipboardSide};
#[cfg(not(feature = "flutter"))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::ui_session_interface::SessionPermissionConfig;

pub use super::lang::*;

#[cfg(not(target_os = "linux"))]
mod audio_playback;
#[cfg(target_os = "windows")]
mod audio_playback_recovery;
#[cfg(all(test, not(target_os = "linux")))]
#[path = "client/tests/audio_state_tests.rs"]
mod audio_state_tests;
pub mod file_trait;
pub mod helper;
pub mod io_loop;
pub mod screenshot;

pub const MILLI1: Duration = Duration::from_millis(1);
pub const SEC30: Duration = Duration::from_secs(30);
// Empirical restart reconnect grace window.
const RESTART_REMOTE_DEVICE_GRACE: Duration = Duration::from_secs(5 * 60);
pub const VIDEO_QUEUE_SIZE: usize = 120;
const MAX_DECODE_FAIL_COUNTER: usize = 3;

pub const LOGIN_MSG_PASSWORD_EMPTY: &str = "Empty Password";
pub const LOGIN_MSG_PASSWORD_WRONG: &str = "Wrong Password";
pub const LOGIN_MSG_2FA_WRONG: &str = "Wrong 2FA Code";
pub const REQUIRE_2FA: &'static str = "2FA Required";
pub const LOGIN_MSG_NO_PASSWORD_ACCESS: &str = "No Password Access";
pub const LOGIN_MSG_OFFLINE: &str = "Offline";
pub const LOGIN_SCREEN_WAYLAND: &str = "Wayland login screen is not supported";
#[cfg(target_os = "linux")]
pub const SCRAP_UBUNTU_HIGHER_REQUIRED: &str = "ubuntu-21-04-required";
#[cfg(target_os = "linux")]
pub const SCRAP_OTHER_VERSION_OR_X11_REQUIRED: &str =
    "wayland-requires-higher-linux-version";
#[cfg(target_os = "linux")]
pub const SCRAP_XDP_PORTAL_UNAVAILABLE: &str =
    "xdp-portal-unavailable";
pub const SCRAP_X11_REQUIRED: &str = "x11 expected";
pub const SCRAP_X11_REF_URL: &str = "";

#[cfg(not(target_os = "linux"))]
pub const AUDIO_BUFFER_MS: usize = 3000;

#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct ClientClipboardContext;

#[cfg(not(feature = "flutter"))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct ClientClipboardContext {
    pub cfg: SessionPermissionConfig,
    pub tx: UnboundedSender<Data>,
    #[cfg(feature = "unix-file-copy-paste")]
    pub is_file_supported: bool,
}

/// Client of the remote desktop.
pub struct Client;

#[cfg(not(target_os = "ios"))]
struct ClipboardState {
    #[cfg(feature = "flutter")]
    is_text_required: bool,
    #[cfg(all(feature = "flutter", feature = "unix-file-copy-paste"))]
    is_file_required: bool,
    running: bool,
}

#[cfg(not(target_os = "linux"))]
lazy_static::lazy_static! {
    static ref AUDIO_HOST: Host = cpal::default_host();
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
lazy_static::lazy_static! {
    static ref ENIGO: Arc<Mutex<enigo::Enigo>> = Arc::new(Mutex::new(enigo::Enigo::new()));
}

#[cfg(not(target_os = "ios"))]
lazy_static::lazy_static! {
    static ref CLIPBOARD_STATE: Arc<Mutex<ClipboardState>> = Arc::new(Mutex::new(ClipboardState::new()));
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn get_key_state(key: enigo::Key) -> bool {
    use enigo::KeyboardControllable;
    #[cfg(target_os = "macos")]
    if key == enigo::Key::NumLock {
        return true;
    }
    ENIGO.lock().unwrap().get_key_state(key)
}

impl Client {
    const CLIENT_CLIPBOARD_NAME: &'static str = "client-clipboard";

    /// Start a new connection.
    pub async fn start(
        peer: &str,
        key: &str,
        token: &str,
        conn_type: ConnType,
        interface: impl Interface,
    ) -> ResultType<((Stream, Option<bool>, Option<Vec<u8>>, &'static str), (i32, String))> {
        debug_assert!(peer == interface.get_id());
        interface.update_received(false);
        match Self::_start(peer, key, token, conn_type, interface.clone()).await {
            Err(err) => {
                let err_str = err.to_string();
                if err_str.starts_with("Failed") {
                    bail!(err_str + ": Please try later");
                } else {
                    return Err(err);
                }
            }
            Ok(x) => Ok(x),
        }
    }

    /// Start a new connection.
    async fn _start(
        peer: &str,
        _key: &str,
        _token: &str,
        conn_type: ConnType,
        interface: impl Interface,
    ) -> ResultType<((Stream, Option<bool>, Option<Vec<u8>>, &'static str), (i32, String))> {
        if config::is_incoming_only() && !is_switch_sides_back(conn_type, &interface).await {
            bail!("Incoming only mode");
        }
        if crate::lan::is_pinned_or_typed(peer) {
            // A typed address is not `lan:<endpoint>/<key>`. It still sends the
            // SealDesk preface. A pinned dial that fails does not fall through.
            let (stream, stream_type, direct_host_key) = hbb_common::tokio::time::timeout(
                std::time::Duration::from_millis(CONNECT_TIMEOUT),
                crate::lan::connect_pinned_or_typed(peer),
            )
            .await??;
            // This slot is the host key negotiated by this dial, and only while it still needs confirmation.
            return Ok((
                (
                    stream,
                    Some(true),
                    direct_host_key.map(String::into_bytes),
                    stream_type,
                ),
                (0, String::new()),
            ));
        }
        // A peer is reached only through its tailcat address; the tunnel does the rest.
        if !crate::tailcat::is_addr(peer) {
            bail!("Unsupported peer {peer:?}: connect with the peer's tailcat address (tc...)");
        }
        if let Some(upgrade) = crate::lan::prepare_tailcat_lan_upgrade(peer).await {
            if !upgrade.delay.is_zero() {
                hbb_common::tokio::time::sleep(upgrade.delay).await;
            }
            match hbb_common::tokio::time::timeout(
                crate::lan::LAN_UPGRADE_TIMEOUT,
                crate::lan::connect(&upgrade.address),
            )
            .await
            {
                Ok(Ok(stream)) => {
                    return Ok(((stream, Some(true), None, "LAN"), (0, String::new())));
                }
                Ok(Err(error)) => {
                    log::info!(
                        "LAN direct failed ({}); using {}",
                        error,
                        crate::lan::transport_after_lan_error(&error.to_string())
                    );
                }
                Err(_) => {
                    log::info!(
                        "LAN direct timed out; using {}",
                        crate::lan::transport_after_lan_error("timed out")
                    );
                }
            }
        }
        let stream = crate::tailcat::connect(peer, CONNECT_TIMEOUT).await?;
        let direct = crate::tailcat::initial_tailcat_direct();
        Ok(((stream, direct, None, crate::tailcat::tailcat_stream_type(direct)), (0, "".to_owned())))
    }

    #[inline]
    #[cfg(feature = "flutter")]
    #[cfg(not(target_os = "ios"))]
    pub fn set_is_text_clipboard_required(b: bool) {
        CLIPBOARD_STATE.lock().unwrap().is_text_required = b;
    }

    #[inline]
    #[cfg(all(feature = "flutter", feature = "unix-file-copy-paste"))]
    pub fn set_is_file_clipboard_required(b: bool) {
        CLIPBOARD_STATE.lock().unwrap().is_file_required = b;
    }

    #[cfg(not(target_os = "ios"))]
    fn try_stop_clipboard() {
        // Disconnected Flutter sessions may keep UI handlers alive, so only connected sessions
        // should block clipboard cleanup.
        #[cfg(feature = "flutter")]
        if crate::flutter::sessions::has_connected_sessions_running(ConnType::DEFAULT_CONN) {
            return;
        }
        #[cfg(not(target_os = "android"))]
        clipboard_listener::unsubscribe(Self::CLIENT_CLIPBOARD_NAME);
        CLIPBOARD_STATE.lock().unwrap().running = false;
        #[cfg(all(feature = "unix-file-copy-paste", target_os = "linux"))]
        if let Err(e) = crate::clipboard::try_empty_clipboard_files_sync(
            crate::clipboard::ClipboardSide::Client,
            0,
        ) {
            log::error!("Failed to empty client clipboard files: {}", e);
        }
        #[cfg(all(feature = "unix-file-copy-paste", target_os = "linux"))]
        clipboard::platform::unix::fuse::uninit_fuse_context(true);
    }

    // `try_start_clipboard` is called by all session when connection is established. (When handling peer info).
    // This function only create one thread with a loop, the loop is shared by all sessions.
    // After all sessions are end, the loop exists.
    //
    // If clipboard update is detected, the text will be sent to all sessions by `send_clipboard_msg`.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn try_start_clipboard(
        _client_clip_ctx: Option<ClientClipboardContext>,
    ) -> Option<UnboundedReceiver<()>> {
        let mut clipboard_lock = CLIPBOARD_STATE.lock().unwrap();
        if clipboard_lock.running {
            return None;
        }

        let (tx_cb_result, rx_cb_result) = mpsc::channel();
        if let Err(e) =
            clipboard_listener::subscribe(Self::CLIENT_CLIPBOARD_NAME.to_owned(), tx_cb_result)
        {
            log::error!("Failed to subscribe clipboard listener: {}", e);
            return None;
        }

        clipboard_lock.running = true;
        let (tx_started, rx_started) = unbounded_channel();

        log::info!("Start client clipboard loop");
        std::thread::spawn(move || {
            let mut handler = ClientClipboardHandler {
                ctx: None,
                #[cfg(not(feature = "flutter"))]
                client_clip_ctx: _client_clip_ctx,
            };

            tx_started.send(()).ok();
            loop {
                if !CLIPBOARD_STATE.lock().unwrap().running {
                    break;
                }
                match rx_cb_result.recv_timeout(Duration::from_millis(CLIPBOARD_INTERVAL)) {
                    Ok(ClipboardEvent::Changed) => {
                        handler.check_clipboard(true);
                    }
                    #[cfg(all(target_os = "linux", feature = "unix-file-copy-paste"))]
                    Ok(ClipboardEvent::InitialSelection) => {
                        // Preserve file startup sync without bypassing text initial-sync settings.
                        handler.check_clipboard(false);
                    }
                    Ok(ClipboardEvent::Stop) => {
                        log::debug!("Clipboard listener stopped");
                        break;
                    }
                    Ok(ClipboardEvent::StopWithError(err)) => {
                        log::error!("Clipboard listener stopped with error: {}", err);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        log::error!("Clipboard listener disconnected");
                        break;
                    }
                }
            }
            log::info!("Stop client clipboard loop");
            CLIPBOARD_STATE.lock().unwrap().running = false;
        });

        Some(rx_started)
    }

    #[cfg(target_os = "android")]
    fn try_start_clipboard(_p: Option<()>) -> Option<UnboundedReceiver<()>> {
        let mut clipboard_lock = CLIPBOARD_STATE.lock().unwrap();
        if clipboard_lock.running {
            return None;
        }
        clipboard_lock.running = true;

        log::info!("Start client clipboard loop");
        std::thread::spawn(move || {
            loop {
                if !CLIPBOARD_STATE.lock().unwrap().running {
                    break;
                }
                if !CLIPBOARD_STATE.lock().unwrap().is_text_required {
                    std::thread::sleep(Duration::from_millis(CLIPBOARD_INTERVAL));
                    continue;
                }

                if let Some(msg) = crate::clipboard::get_clipboards_msg(true) {
                    crate::flutter::send_clipboard_msg(msg, false);
                }

                std::thread::sleep(Duration::from_millis(CLIPBOARD_INTERVAL));
            }
            log::info!("Stop client clipboard loop");
            CLIPBOARD_STATE.lock().unwrap().running = false;
        });

        None
    }
}

#[cfg(not(target_os = "ios"))]
impl ClipboardState {
    fn new() -> Self {
        Self {
            #[cfg(feature = "flutter")]
            is_text_required: true,
            #[cfg(all(feature = "flutter", feature = "unix-file-copy-paste"))]
            is_file_required: true,
            running: false,
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct ClientClipboardHandler {
    ctx: Option<crate::clipboard::ClipboardContext>,
    #[cfg(not(feature = "flutter"))]
    client_clip_ctx: Option<ClientClipboardContext>,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl ClientClipboardHandler {
    fn is_text_required(&self) -> bool {
        #[cfg(feature = "flutter")]
        {
            CLIPBOARD_STATE.lock().unwrap().is_text_required
        }
        #[cfg(not(feature = "flutter"))]
        {
            self.client_clip_ctx
                .as_ref()
                .map(|ctx| ctx.cfg.is_text_clipboard_required())
                .unwrap_or(false)
        }
    }

    #[cfg(feature = "unix-file-copy-paste")]
    fn is_file_required(&self) -> bool {
        #[cfg(feature = "flutter")]
        {
            CLIPBOARD_STATE.lock().unwrap().is_file_required
        }
        #[cfg(not(feature = "flutter"))]
        {
            self.client_clip_ctx
                .as_ref()
                .map(|ctx| ctx.cfg.is_file_clipboard_required())
                .unwrap_or(false)
        }
    }

    fn check_clipboard(&mut self, check_text: bool) {
        if CLIPBOARD_STATE.lock().unwrap().running {
            #[cfg(feature = "unix-file-copy-paste")]
            if self.is_file_required() {
                if let Some(urls) = check_clipboard_files(&mut self.ctx, ClipboardSide::Client, false) {
                    if !urls.is_empty() {
                        #[cfg(target_os = "macos")]
                        if crate::clipboard::is_file_url_set_by_rustdesk(&urls) {
                            return;
                        }
                        match clipboard::platform::unix::serv_files::sync_files(&urls) {
                            Ok(()) => {
                                let msg = crate::clipboard_file::clip_2_msg(
                                    unix_file_clip::get_format_list(),
                                );
                                self.send_msg(msg, true);
                            }
                            Err(e) => {
                                log::error!("Failed to sync clipboard files: {}", e);
                            }
                        }
                        return;
                    }
                }
            }

            if check_text && self.is_text_required() {
                if let Some(msg) = check_clipboard(&mut self.ctx, ClipboardSide::Client, false) {
                    self.send_msg(msg, false);
                }
            }
        }
    }

    #[inline]
    #[cfg(feature = "flutter")]
    fn send_msg(&self, msg: Message, _is_file: bool) {
        crate::flutter::send_clipboard_msg(msg, _is_file);
    }

    #[cfg(not(feature = "flutter"))]
    fn send_msg(&self, msg: Message, _is_file: bool) {
        if let Some(ctx) = &self.client_clip_ctx {
            #[cfg(feature = "unix-file-copy-paste")]
            if _is_file {
                if ctx.is_file_supported {
                    let _ = ctx.tx.send(Data::Message(msg));
                }
                return;
            }

            let pi = ctx.cfg.lc.read().unwrap().peer_info.clone();
            if let Some(pi) = pi.as_ref() {
                if let Some(message::Union::MultiClipboards(multi_clipboards)) = &msg.union {
                    if let Some(msg_out) = crate::clipboard::get_msg_if_not_support_multi_clip(
                        &pi.version,
                        &pi.platform,
                        multi_clipboards,
                    ) {
                        let _ = ctx.tx.send(Data::Message(msg_out));
                        return;
                    }
                }
            }
            let _ = ctx.tx.send(Data::Message(msg));
        }
    }
}

/// Audio handler for the [`Client`].
#[derive(Default)]
pub struct AudioHandler {
    audio_decoder: Option<(AudioDecoder, Vec<f32>)>,
    #[cfg(target_os = "linux")]
    simple: Option<psimple::Simple>,
    #[cfg(not(target_os = "linux"))]
    audio_buffer: AudioBuffer,
    #[cfg(not(target_os = "linux"))]
    audio_resampler: Option<crate::audio_resampler::AudioResampler>,
    sample_rate: (u32, u32),
    #[cfg(not(target_os = "linux"))]
    audio_stream: Option<Box<dyn StreamTrait>>,
    channels: u16,
    #[cfg(not(target_os = "linux"))]
    device_channel: u16,
    #[cfg(not(target_os = "linux"))]
    playback_status: Arc<audio_playback::AudioPlaybackStatus>,
    #[cfg(target_os = "windows")]
    playback_recovery: audio_playback_recovery::PlaybackRecovery,
}

#[cfg(not(target_os = "linux"))]
#[derive(Clone, Copy)]
struct DecodedAudioConfig {
    sample_rate: u32,
    input_channels: u16,
    output_channels: u16,
}

#[cfg(not(target_os = "linux"))]
fn create_audio_resampler(
    input_rate: u32,
    output_rate: u32,
    channels: u16,
) -> ResultType<Option<crate::audio_resampler::AudioResampler>> {
    if input_rate == output_rate {
        return Ok(None);
    }
    Ok(Some(crate::audio_resampler::AudioResampler::new(
        crate::audio_resampler::AudioResamplerConfig {
            input_rate,
            output_rate,
            channels,
        },
    )?))
}

#[cfg(not(target_os = "linux"))]
fn prepare_decoded_audio(
    input: &[f32],
    resampler: Option<&mut crate::audio_resampler::AudioResampler>,
    config: DecodedAudioConfig,
) -> Result<Vec<f32>, crate::audio_resampler::AudioResamplerError> {
    let mut output = match resampler {
        Some(resampler) => resampler.process(input)?,
        None => input.to_owned(),
    };
    if config.input_channels != config.output_channels {
        output = crate::audio_rechannel(
            output,
            config.sample_rate,
            config.sample_rate,
            config.input_channels,
            config.output_channels,
        );
    }
    Ok(output)
}

#[cfg(not(target_os = "linux"))]
struct AudioBuffer(
    pub Arc<std::sync::Mutex<ringbuf::HeapRb<f32>>>,
    usize,
    [usize; 30],
    Arc<std::sync::atomic::AtomicUsize>,
);

#[cfg(not(target_os = "linux"))]
impl Default for AudioBuffer {
    fn default() -> Self {
        Self(
            Arc::new(std::sync::Mutex::new(
                ringbuf::HeapRb::<f32>::new(48000 * 2 * AUDIO_BUFFER_MS / 1000), // 48000hz, 2 channel
            )),
            48000 * 2,
            [0; 30],
            Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        )
    }
}

#[cfg(not(target_os = "linux"))]
impl AudioBuffer {
    pub fn resize(&mut self, sample_rate: usize, channels: usize) {
        let capacity = sample_rate * channels * AUDIO_BUFFER_MS / 1000;
        let old_capacity = self.0.lock().unwrap().capacity();
        if capacity != old_capacity {
            *self.0.lock().unwrap() = ringbuf::HeapRb::<f32>::new(capacity);
            self.1 = sample_rate * channels;
            log::info!("Audio buffer resized from {old_capacity} to {capacity}");
        }
    }

    fn try_shrink(&mut self, having: usize) {
        extern crate chrono;
        use chrono::prelude::*;

        let mut i = (having * 10) / self.1;
        if i > 29 {
            i = 29;
        }
        self.2[i] += 1;

        #[allow(non_upper_case_globals)]
        static mut tms: i64 = 0;
        let dt = Local::now().timestamp_millis();
        unsafe {
            if tms == 0 {
                tms = dt;
                return;
            } else if dt < tms + 12000 {
                return;
            }
            tms = dt;
        }

        // the safer water mark to drop
        let mut zero = 0;
        // the water mark taking most of time
        let mut max = 0;
        for i in 0..30 {
            if self.2[i] == 0 && zero == i {
                zero += 1;
            }

            if self.2[i] > self.2[max] {
                self.2[max] = 0;
                max = i;
            } else {
                self.2[i] = 0;
            }
        }
        zero = zero * 2 / 3;

        // how many data can be dropped:
        // 1. will not drop if buffered data is less than 600ms
        // 2. choose based on min(zero, max)
        const N: usize = 4;
        self.2[max] = 0;
        if max < 6 {
            return;
        } else if max > zero * N {
            max = zero * N;
        }

        let mut lock = self.0.lock().unwrap();
        let cap = lock.capacity();
        let having = lock.occupied_len();
        let skip = (cap * max / (30 * N) + 1) & (!1);
        if (having > skip * 3) && (skip > 0) {
            lock.skip(skip);
            let generation = self.signal_discontinuity();
            drop(lock);
            log::info!("skip {skip}, based {max} {zero}, generation={generation}");
        }
    }

    /// The caller must hold the PCM buffer lock while signaling the discard.
    fn signal_discontinuity(&self) -> usize {
        self.3
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1)
    }

    /// append pcm to audio buffer, if buffered data
    /// exceeds AUDIO_BUFFER_MS,  only AUDIO_BUFFER_MS
    /// will be kept.
    fn append_pcm2(&self, buffer: &[f32]) -> usize {
        let mut lock = self.0.lock().unwrap();
        let cap = lock.capacity();
        let having = lock.occupied_len() + buffer.len();
        lock.push_slice_overwrite(buffer);
        let discard = (having > cap).then(|| (having - cap, self.signal_discontinuity()));
        let occupied = lock.occupied_len();
        drop(lock);
        if let Some((discarded, generation)) = discard {
            hbb_common::throttled_log!(
                audio_playback::AUDIO_PLAYBACK_LOG_INTERVAL,
                debug,
                "Audio buffer capacity discard: samples={discarded}, generation={generation}"
            );
        }
        occupied
    }

    /// append pcm to audio buffer, trying to drop data
    /// when data is too much (per 12 seconds) based
    /// statistics.
    pub fn append_pcm(&mut self, buffer: &[f32]) {
        let having = self.append_pcm2(buffer);
        self.try_shrink(having);
    }
}

impl AudioHandler {
    #[cfg(target_os = "linux")]
    fn start_audio(&mut self, format0: AudioFormat) -> ResultType<()> {
        use psimple::Simple;
        use pulse::sample::{Format, Spec};
        use pulse::stream::Direction;

        let spec = Spec {
            format: Format::F32le,
            channels: format0.channels as _,
            rate: format0.sample_rate as _,
        };
        if !spec.is_valid() {
            bail!("Invalid audio format");
        }

        self.simple = Some(Simple::new(
            None,                   // Use the default server
            &crate::get_app_name(), // Our application’s name
            Direction::Playback,    // We want a playback stream
            None,                   // Use the default device
            "playback",             // Description of our stream
            &spec,                  // Our sample format
            None,                   // Use default channel map
            None,                   // Use default buffering attributes
        )?);
        self.sample_rate = (format0.sample_rate, format0.sample_rate);
        Ok(())
    }

    /// Start the audio playback.
    #[cfg(not(target_os = "linux"))]
    fn start_audio(&mut self, format0: AudioFormat) -> ResultType<()> {
        let device = AUDIO_HOST
            .default_output_device()
            .with_context(|| "Failed to get default output device")?;
        log::info!(
            "Using default output device: \"{}\"",
            device.name().unwrap_or("".to_owned())
        );
        let config = device.default_output_config().map_err(|e| anyhow!(e))?;
        let sample_format = config.sample_format();
        log::info!("Default output format: {:?}", config);
        log::info!("Remote input format: {:?}", format0);
        #[allow(unused_mut)]
        let mut config: StreamConfig = config.into();
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            // this makes ios and android audio output not work
            config.buffer_size = cpal::BufferSize::Fixed(64);
        }

        self.sample_rate = (format0.sample_rate, config.sample_rate.0);
        let audio_resampler = create_audio_resampler(
            format0.sample_rate, config.sample_rate.0, format0.channels as _,
        )?;
        let mut build_output_stream = |config: StreamConfig| match sample_format {
            cpal::SampleFormat::I8 => self.build_output_stream::<i8>(&config, &device),
            cpal::SampleFormat::I16 => self.build_output_stream::<i16>(&config, &device),
            cpal::SampleFormat::I32 => self.build_output_stream::<i32>(&config, &device),
            cpal::SampleFormat::I64 => self.build_output_stream::<i64>(&config, &device),
            cpal::SampleFormat::U8 => self.build_output_stream::<u8>(&config, &device),
            cpal::SampleFormat::U16 => self.build_output_stream::<u16>(&config, &device),
            cpal::SampleFormat::U32 => self.build_output_stream::<u32>(&config, &device),
            cpal::SampleFormat::U64 => self.build_output_stream::<u64>(&config, &device),
            cpal::SampleFormat::F32 => self.build_output_stream::<f32>(&config, &device),
            cpal::SampleFormat::F64 => self.build_output_stream::<f64>(&config, &device),
            f => bail!("unsupported audio format: {:?}", f),
        };
        if config.channels > format0.channels as _ {
            let no_rechannel_config = StreamConfig {
                channels: format0.channels as _,
                ..config.clone()
            };
            if let Err(_) = build_output_stream(no_rechannel_config) {
                build_output_stream(config)?;
            }
        } else {
            build_output_stream(config)?;
        }
        self.audio_resampler = audio_resampler;

        Ok(())
    }

    /// Handle audio format and create an audio decoder.
    pub fn handle_format(&mut self, f: AudioFormat) {
        self.handle_format_with_start(f, Self::start_audio);
    }

    fn handle_format_with_start(
        &mut self,
        f: AudioFormat,
        start: impl FnOnce(&mut Self, AudioFormat) -> ResultType<()>,
    ) {
        if !is_supported_audio_channel_count(f.channels) {
            log::error!("Unsupported audio channel count: {}", f.channels);
            return;
        }
        match AudioDecoder::new(f.sample_rate, if f.channels > 1 { Stereo } else { Mono }) {
            Ok(d) => {
                #[cfg(target_os = "windows")]
                let playback_failed = self.cancel_pending_playback();
                #[cfg(target_os = "linux")]
                let keep_existing_stream = self.simple.is_some()
                    && self.sample_rate.0 == f.sample_rate
                    && u32::from(self.channels) == f.channels;
                #[cfg(not(target_os = "linux"))]
                let keep_existing_stream = self.audio_stream.is_some()
                    && self.sample_rate.0 == f.sample_rate
                    && u32::from(self.channels) == f.channels;
                let buffer = vec![0.; f.sample_rate as usize * f.channels as usize];
                #[cfg(not(target_os = "linux"))]
                let mut previous = std::mem::take(self);
                #[cfg(target_os = "windows")]
                self.prepare_playback(&f);
                self.audio_decoder = Some((d, buffer));
                self.channels = f.channels as _;
                let result = start(self, f);
                #[cfg(target_os = "windows")]
                let keep_existing_stream = keep_existing_stream
                    && !playback_failed
                    && !previous.playback_recovery.report_pending();
                #[cfg(not(target_os = "linux"))]
                if result.is_err() && keep_existing_stream {
                    // The restarted capture has new Opus history even when output startup fails.
                    previous.audio_decoder = self.audio_decoder.take();
                    *self = previous;
                    self.handle_audio_start_result(result, true);
                    return;
                }
                #[cfg(target_os = "windows")]
                self.finish_playback_replacement(result, keep_existing_stream.then_some(previous));
                #[cfg(not(target_os = "windows"))]
                self.handle_audio_start_result(result, keep_existing_stream);
            }
            Err(err) => {
                log::error!("Failed to create audio decoder: {}", err);
            }
        }
    }

    fn handle_audio_start_result(&mut self, result: ResultType<()>, keep_existing_stream: bool) {
        if let Err(error) = result {
            if keep_existing_stream {
                log::error!(
                    "Failed to replace audio playback stream; keeping the existing compatible stream: {error:#}"
                );
            } else {
                *self = Self::default();
                log::error!("Failed to start audio playback: {error:#}");
            }
        }
    }

    /// Handle audio frame and play it.
    #[inline]
    pub fn handle_frame(&mut self, frame: AudioFrame) {
        #[cfg(not(target_os = "linux"))]
        self.playback_status.report_errors();
        #[cfg(not(target_os = "linux"))]
        if self.audio_stream.is_none()
            || !self
                .playback_status
                .ready
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return;
        }
        #[cfg(target_os = "linux")]
        if self.simple.is_none() {
            log::trace!("PulseAudio simple binding does not exists");
            return;
        }
        self.audio_decoder.as_mut().map(|(d, buffer)| {
            let decoded_frames = match d.decode_float(&frame.data, buffer, false) {
                Ok(decoded_frames) => decoded_frames,
                Err(error) => {
                    log::warn!("Failed to decode audio frame: {error:?}");
                    return;
                }
            };
            let channels = self.channels;
            let n = decoded_frames * channels as usize;
            #[cfg(not(target_os = "linux"))]
            {
                let config = DecodedAudioConfig {
                    sample_rate: self.sample_rate.1,
                    input_channels: self.channels,
                    output_channels: self.device_channel,
                };
                let buffer = match prepare_decoded_audio(
                    &buffer[0..n],
                    self.audio_resampler.as_mut(),
                    config,
                ) {
                    Ok(output) => output,
                    Err(error) => {
                        log::error!("Failed to resample decoded audio: {error:#}");
                        return;
                    }
                };
                self.audio_buffer.append_pcm(&buffer);
            }
            #[cfg(target_os = "linux")]
            {
                let data_u8 =
                    unsafe { std::slice::from_raw_parts::<u8>(buffer.as_ptr() as _, n * 4) };
                self.simple.as_mut().map(|x| x.write(data_u8));
            }
        });
    }

    /// Build audio output stream for current device.
    #[cfg(not(target_os = "linux"))]
    fn build_output_stream<T: cpal::Sample + cpal::SizedSample + cpal::FromSample<f32>>(
        &mut self,
        config: &StreamConfig,
        device: &Device,
    ) -> ResultType<()> {
        self.device_channel = config.channels;
        #[cfg(target_os = "windows")]
        let err_fn = self.playback_recovery.new_error_callback();
        #[cfg(not(target_os = "windows"))]
        let err_fn = move |err| {
            // too many errors, will improve later
            log::trace!("an error occurred on stream: {}", err);
        };
        self.audio_buffer
            .resize(config.sample_rate.0 as _, config.channels as _);
        let audio_buffer = self.audio_buffer.0.clone();
        let discontinuity_generation = self.audio_buffer.3.clone();
        let mut playback_writer = audio_playback::AudioPlaybackWriter::new(
            audio_playback::AudioPlaybackConfig {
                sample_rate: config.sample_rate.0,
                channels: config.channels as usize,
            },
            audio_buffer,
            discontinuity_generation,
        )?;
        let playback_status = playback_writer.status.clone();
        let timeout = None;
        let stream = device.build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                playback_writer.write_output(data);
            },
            err_fn,
            timeout,
        )?;
        stream.play()?;
        self.audio_stream = Some(Box::new(stream));
        self.playback_status = playback_status;
        Ok(())
    }
}

fn is_supported_audio_channel_count(channels: u32) -> bool {
    (1..=2).contains(&channels)
}

#[cfg(test)]
mod audio_format_tests {
    use super::is_supported_audio_channel_count;

    #[test]
    fn only_mono_and_stereo_are_supported() {
        assert!(is_supported_audio_channel_count(1));
        assert!(is_supported_audio_channel_count(2));
        assert!(!is_supported_audio_channel_count(0));
        assert!(!is_supported_audio_channel_count(u32::MAX));
    }

    #[test]
    fn failed_audio_start_discards_format_state() {
        use super::{anyhow, AudioDecoder, AudioHandler, Stereo};

        const SAMPLE_RATE: u32 = 48_000;
        const CHANNELS: u16 = 2;
        let decoder = AudioDecoder::new(SAMPLE_RATE, Stereo).unwrap();
        let mut handler = AudioHandler {
            audio_decoder: Some((decoder, Vec::new())),
            sample_rate: (SAMPLE_RATE, SAMPLE_RATE),
            channels: CHANNELS,
            ..Default::default()
        };

        handler.handle_audio_start_result(Err(anyhow!("Injected playback startup failure")), false);

        assert!(handler.audio_decoder.is_none());
        assert_eq!(handler.channels, 0);
        assert_eq!(handler.sample_rate, (0, 0));
    }
}

/// Video handler for the [`Client`].
pub struct VideoHandler {
    decoder: Decoder,
    pub rgb: ImageRgb,
    pub texture: ImageTexture,
    recorder: Arc<Mutex<Option<Recorder>>>,
    record: bool,
    _display: usize, // useful for debug
    fail_counter: usize,
    first_frame: bool,
}

impl VideoHandler {
    #[cfg(feature = "flutter")]
    pub fn get_adapter_luid() -> Option<i64> {
        crate::flutter::get_adapter_luid()
    }

    #[cfg(not(feature = "flutter"))]
    pub fn get_adapter_luid() -> Option<i64> {
        None
    }

    /// Create a new video handler.
    pub fn new(format: CodecFormat, _display: usize) -> Self {
        let luid = Self::get_adapter_luid();
        log::info!("new video handler for display #{_display}, format: {format:?}, luid: {luid:?}");
        let rgba_format =
            if cfg!(feature = "flutter") && (cfg!(windows) || cfg!(target_os = "linux")) {
                ImageFormat::ABGR
            } else {
                ImageFormat::ARGB
            };
        VideoHandler {
            decoder: Decoder::new(format, luid),
            rgb: ImageRgb::new(rgba_format, crate::get_dst_align_rgba()),
            texture: Default::default(),
            recorder: Default::default(),
            record: false,
            _display,
            fail_counter: 0,
            first_frame: true,
        }
    }

    /// Handle a new video frame.
    #[inline]
    pub fn handle_frame(
        &mut self,
        vf: VideoFrame,
        pixelbuffer: &mut bool,
        chroma: &mut Option<Chroma>,
    ) -> ResultType<bool> {
        let format = CodecFormat::from(&vf);
        if format != self.decoder.format() {
            self.reset(Some(format));
        }
        match &vf.union {
            Some(frame) => {
                let res = self.decoder.handle_video_frame(
                    frame,
                    &mut self.rgb,
                    &mut self.texture,
                    pixelbuffer,
                    chroma,
                );
                if res.as_ref().is_ok_and(|x| *x) {
                    self.fail_counter = 0;
                } else {
                    if self.fail_counter < usize::MAX {
                        if self.first_frame && self.fail_counter < MAX_DECODE_FAIL_COUNTER {
                            log::error!("decode first frame failed");
                            self.fail_counter = MAX_DECODE_FAIL_COUNTER;
                        } else {
                            self.fail_counter += 1;
                        }
                        log::error!(
                            "Failed to handle video frame, fail counter: {}",
                            self.fail_counter
                        );
                    }
                }
                self.first_frame = false;
                if self.record {
                    self.recorder.lock().unwrap().as_mut().map(|r| {
                        let (w, h) = if *pixelbuffer {
                            (self.rgb.w, self.rgb.h)
                        } else {
                            (self.texture.w, self.texture.h)
                        };
                        r.write_frame(frame, w, h).ok();
                    });
                }
                res
            }
            _ => Ok(false),
        }
    }

    /// Reset the decoder, change format if it is Some
    pub fn reset(&mut self, format: Option<CodecFormat>) {
        log::info!(
            "reset video handler for display #{}, format: {format:?}",
            self._display
        );
        #[cfg(target_os = "macos")]
        self.rgb.set_align(crate::get_dst_align_rgba());
        let luid = Self::get_adapter_luid();
        let format = format.unwrap_or(self.decoder.format());
        self.decoder = Decoder::new(format, luid);
        self.fail_counter = 0;
        self.first_frame = true;
    }

    /// Start or stop screen record.
    pub fn record_screen(&mut self, start: bool, id: String, display_idx: usize, camera: bool) {
        self.record = false;
        if start {
            self.recorder = Recorder::new(RecorderContext {
                server: false,
                id,
                dir: crate::ui_interface::video_save_directory(false),
                display_idx,
                camera,
                tx: None,
            })
            .map_or(Default::default(), |r| Arc::new(Mutex::new(Some(r))));
        } else {
            self.recorder = Default::default();
        }

        self.record = start;
    }
}

// The source of sent password
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum PasswordSource {
    PersonalAb(Vec<u8>),
    SharedAb(String),
    Undefined,
    /// Password supplied with this connect (home-page field or pairing code).
    /// It must not replace a saved password unless the user opts in.
    Preset,
}

impl Default for PasswordSource {
    fn default() -> Self {
        PasswordSource::Undefined
    }
}

impl PasswordSource {
    /// Connect preset (typed password or pairing code), not a saved peer password.
    fn is_preset(&self) -> bool {
        matches!(self, PasswordSource::Preset)
    }

    // Whether the password is personal ab password
    pub fn is_personal_ab(&self, password: &[u8]) -> bool {
        if password.is_empty() {
            return false;
        }
        match self {
            PasswordSource::PersonalAb(p) => p == password,
            _ => false,
        }
    }

    // Whether the password is shared ab password
    pub fn is_shared_ab(&self, password: &[u8], hash: &Hash) -> bool {
        if password.is_empty() {
            return false;
        }
        match self {
            PasswordSource::SharedAb(p) => Self::equal(p, password, hash),
            _ => false,
        }
    }

    //  Whether the password equals to the connected password
    fn equal(password: &str, connected_password: &[u8], hash: &Hash) -> bool {
        let mut hasher = Sha256::new();
        hasher.update(password);
        hasher.update(&hash.salt);
        let res = hasher.finalize();
        connected_password[..] == res[..]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ConnToken {
    password: Vec<u8>,
    password_source: PasswordSource,
    session_id: u64,
}

/// Login config handler for [`Client`].
#[derive(Default)]
pub struct LoginConfigHandler {
    id: String,
    pub conn_type: ConnType,
    pub is_terminal_admin: bool,
    hash: Hash,
    password: Vec<u8>, // remember password for reconnect
    pub remember: bool,
    config: PeerConfig,
    pub port_forward: (String, i32),
    /// This login's `multiplex`, filled with `port_forward` under the turn
    /// lock. `port_forward_mux` says whether a mapping probes for the tunnel;
    /// one the probe latched to the raw pipe logs in without asking, so an
    /// upgraded peer keeps giving it the raw pipe.
    pub(crate) port_forward_multiplex: bool,
    /// Set once per window, before its mappings start: every accept's claim
    /// reads it.
    pub(crate) port_forward_mux: bool,
    /// Held by a port-forward mapping from filling `port_forward` and `hash`
    /// until its login is built from them; a window's mappings log in
    /// concurrently.
    pub(crate) port_forward_login_turn: Arc<hbb_common::tokio::sync::Mutex<()>>,
    pub version: i64,
    features: Option<Features>,
    pub session_id: u64, // used for local <-> server communication
    pub supported_encoding: SupportedEncoding,
    restarting_remote_device: bool,
    // Start time of the restart grace window. On Windows the peer may briefly
    // reconnect before the real reboot disconnect.
    restart_remote_device_at: Option<Instant>,
    pub received: bool,
    switch_uuid: Option<String>,
    #[cfg(feature = "flutter")]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    switch_back_allowed: bool,
    pub save_ab_password_to_recent: bool, // true: connected with ab password
    pub custom_fps: Arc<Mutex<Option<usize>>>,
    pub last_auto_fps: Option<usize>,
    pub adapter_luid: Option<i64>,
    pub mark_unsupported: Vec<CodecFormat>,
    pub selected_windows_session_id: Option<u32>,
    pub peer_info: Option<PeerInfo>,
    password_source: PasswordSource, // where the sent password comes from
    shared_password: Option<String>, // Store the shared password
    pub enable_trusted_devices: bool,
    pub record_state: bool,
    pub record_permission: bool,
}

impl Deref for LoginConfigHandler {
    type Target = PeerConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl LoginConfigHandler {
    pub(crate) fn set_hash(&mut self, hash: Hash) {
        self.hash = hash;
    }

    /// Initialize the login config handler.
    ///
    /// # Arguments
    ///
    /// * `id` - id of peer
    /// * `conn_type` - Connection type enum.
    pub fn initialize(
        &mut self,
        id: String,
        conn_type: ConnType,
        switch_uuid: Option<String>,
        adapter_luid: Option<i64>,
        shared_password: Option<String>,
        conn_token: Option<String>,
    ) {
        self.id = id;
        self.conn_type = conn_type;
        let config = self.load_config();
        self.remember = !config.password.is_empty();
        self.config = config;

        let conn_token = conn_token
            .map(|x| serde_json::from_str::<ConnToken>(&x).ok())
            .flatten();
        let mut sid = 0;
        if let Some(token) = conn_token {
            sid = token.session_id;
            self.password = token.password; // use as last password
            self.password_source = token.password_source;
        }
        if sid == 0 {
            sid = rand::random();
            if sid == 0 {
                // you won the lottery
                sid = 1;
            }
        }
        self.session_id = sid;
        self.supported_encoding = Default::default();
        self.clear_restarting_remote_device();
        self.received = false;
        #[cfg(feature = "flutter")]
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            self.switch_back_allowed = false;
        }
        self.switch_uuid = switch_uuid;
        self.adapter_luid = adapter_luid;
        self.selected_windows_session_id = None;
        self.shared_password = shared_password;
        self.record_state = false;
        self.record_permission = true;

        // `std::env::remove_var("IS_TERMINAL_ADMIN");` is called in `session_add_sync()` - `flutter_ffi.rs`.
        let is_terminal_admin = conn_type == ConnType::TERMINAL
            && std::env::var("IS_TERMINAL_ADMIN").map_or(false, |v| v == "Y");
        self.is_terminal_admin = is_terminal_admin;
    }

    #[cfg(feature = "flutter")]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn allow_switch_back_once(&mut self) {
        self.switch_back_allowed = true;
    }

    #[cfg(feature = "flutter")]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub fn consume_switch_back_permission(&mut self) -> bool {
        if self.switch_back_allowed {
            self.switch_back_allowed = false;
            true
        } else {
            false
        }
    }

    /// Check if the client should auto login.
    /// Return password if the client should auto login, otherwise return empty string.
    pub fn should_auto_login(&self) -> String {
        let l = self.lock_after_session_end.v;
        let a = !self.get_option("auto-login").is_empty();
        let p = self.get_option("os-password");
        if !p.is_empty() && l && a {
            p
        } else {
            "".to_owned()
        }
    }

    /// Load [`PeerConfig`].
    pub fn load_config(&self) -> PeerConfig {
        debug_assert!(self.id.len() > 0);
        PeerConfig::load(&self.id)
    }

    /// Save a [`PeerConfig`] into the handler.
    ///
    /// # Arguments
    ///
    /// * `config` - [`PeerConfig`] to save.
    pub fn save_config(&mut self, config: PeerConfig) {
        config.store(&self.id);
        self.config = config;
    }

    /// Set an option for handler's [`PeerConfig`].
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    /// * `v` - value of option
    pub fn set_option(&mut self, k: String, v: String) {
        let mut config = self.load_config();
        if v == self.get_option(&k) {
            return;
        }
        config.options.insert(k, v);
        self.save_config(config);
    }

    //to-do: too many dup code below.

    /// Save view style to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The view style to be saved.
    pub fn save_view_style(&mut self, value: String) {
        let mut config = self.load_config();
        config.view_style = value;
        self.save_config(config);
    }

    /// Save keyboard mode to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The view style to be saved.
    pub fn save_keyboard_mode(&mut self, value: String) {
        let mut config = self.load_config();
        config.keyboard_mode = value;
        self.save_config(config);
    }

    /// Save reverse mouse wheel ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The reverse mouse wheel ("", "Y").
    pub fn save_reverse_mouse_wheel(&mut self, value: String) {
        let mut config = self.load_config();
        config.reverse_mouse_wheel = value;
        self.save_config(config);
    }

    /// Save "displays_as_individual_windows" ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The "displays_as_individual_windows" value ("", "Y").
    pub fn save_displays_as_individual_windows(&mut self, value: String) {
        let mut config = self.load_config();
        config.displays_as_individual_windows = value;
        self.save_config(config);
    }

    /// Save "use_all_my_displays_for_the_remote_session" ("", "Y") to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The "use_all_my_displays_for_the_remote_session" value ("", "Y").
    pub fn save_use_all_my_displays_for_the_remote_session(&mut self, value: String) {
        let mut config = self.load_config();
        config.use_all_my_displays_for_the_remote_session = value;
        self.save_config(config);
    }

    /// Save scroll style to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The scroll style to be saved.
    pub fn save_scroll_style(&mut self, value: String) {
        let mut config = self.load_config();
        config.scroll_style = value;
        self.save_config(config);
    }

    /// Save edge scroll edge thickness to the current config.
    ///
    /// # Arguments
    ///
    /// * `value` - The edge thickness to be saved.
    pub fn save_edge_scroll_edge_thickness(&mut self, value: i32) {
        let mut config = self.load_config();
        config.edge_scroll_edge_thickness = value;
        self.save_config(config);
    }

    /// Set a ui config of flutter for handler's [`PeerConfig`].
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    /// * `v` - value of option
    pub fn save_ui_flutter(&mut self, k: String, v: String) {
        let mut config = self.load_config();
        if v.is_empty() {
            config.ui_flutter.remove(&k);
        } else {
            config.ui_flutter.insert(k, v);
        }
        self.save_config(config);
    }

    /// Get a ui config of flutter for handler's [`PeerConfig`].
    /// Return String if the option is found, otherwise return "".
    ///
    /// # Arguments
    ///
    /// * `k` - key of option
    pub fn get_ui_flutter(&self, k: &str) -> String {
        if let Some(v) = self.config.ui_flutter.get(k) {
            v.clone()
        } else {
            "".to_owned()
        }
    }

    /// Toggle an option in the handler.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the option to toggle.
    ///
    // It's Ok to check the option empty in this function.
    // `toggle_option()` is only called in a session.
    // Custom client advanced settings will not effect this function.
    pub fn toggle_option(&mut self, name: String) -> Option<Message> {
        let mut option = OptionMessage::default();
        let mut config = self.load_config();
        if name == "show-remote-cursor" {
            config.show_remote_cursor.v = !config.show_remote_cursor.v;
            option.show_remote_cursor = (if config.show_remote_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "follow-remote-cursor" {
            config.follow_remote_cursor.v = !config.follow_remote_cursor.v;
            option.follow_remote_cursor = (if config.follow_remote_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "follow-remote-window" {
            config.follow_remote_window.v = !config.follow_remote_window.v;
            option.follow_remote_window = (if config.follow_remote_window.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "disable-audio" {
            config.disable_audio.v = !config.disable_audio.v;
            option.disable_audio = (if config.disable_audio.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "disable-clipboard" {
            config.disable_clipboard.v = !config.disable_clipboard.v;
            option.disable_clipboard = (if config.disable_clipboard.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "lock-after-session-end" {
            config.lock_after_session_end.v = !config.lock_after_session_end.v;
            option.lock_after_session_end = (if config.lock_after_session_end.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == keys::OPTION_TERMINAL_PERSISTENT {
            config.terminal_persistent.v = !config.terminal_persistent.v;
            option.terminal_persistent = (if config.terminal_persistent.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "privacy-mode" {
            // try toggle privacy mode
            option.privacy_mode = (if config.privacy_mode.v {
                BoolOption::No
            } else {
                BoolOption::Yes
            })
            .into();
        } else if name == "enable-file-copy-paste" {
            config.enable_file_copy_paste.v = !config.enable_file_copy_paste.v;
            option.enable_file_transfer = (if config.enable_file_copy_paste.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            })
            .into();
        } else if name == "block-input" {
            option.block_input = BoolOption::Yes.into();
        } else if name == "unblock-input" {
            option.block_input = BoolOption::No.into();
        } else if name == "show-quality-monitor" {
            config.show_quality_monitor.v = !config.show_quality_monitor.v;
        } else if name == "allow_swap_key" {
            config.allow_swap_key.v = !config.allow_swap_key.v;
        } else if name == "view-only" {
            config.view_only.v = !config.view_only.v;
            let f = |b: bool| {
                if b {
                    BoolOption::Yes.into()
                } else {
                    BoolOption::No.into()
                }
            };
            if config.view_only.v {
                option.disable_keyboard = f(true);
                option.disable_clipboard = f(true);
                option.show_remote_cursor = f(true);
                option.enable_file_transfer = f(false);
                option.lock_after_session_end = f(false);
            } else {
                option.disable_keyboard = f(false);
                option.disable_clipboard = f(self.get_toggle_option("disable-clipboard"));
                option.show_remote_cursor = f(self.get_toggle_option("show-remote-cursor"));
                option.enable_file_transfer = f(self.config.enable_file_copy_paste.v);
                option.lock_after_session_end = f(self.config.lock_after_session_end.v);
                if config.show_my_cursor.v {
                    config.show_my_cursor.v = false;
                    option.show_my_cursor = BoolOption::No.into();
                }
            }
        } else if name == "show-my-cursor" {
            config.show_my_cursor.v = !config.show_my_cursor.v;
            option.show_my_cursor = if config.show_my_cursor.v {
                BoolOption::Yes
            } else {
                BoolOption::No
            }
            .into();
        } else {
            let is_set = self
                .options
                .get(&name)
                .map(|o| !o.is_empty())
                .unwrap_or(false);
            if is_set {
                self.config.options.remove(&name);
            } else {
                self.config.options.insert(name, "Y".to_owned());
            }
            self.config.store(&self.id);
            return None;
        }

        #[cfg(feature = "unix-file-copy-paste")]
        if option.enable_file_transfer.enum_value() == Ok(BoolOption::No) {
            crate::clipboard::try_empty_clipboard_files(crate::clipboard::ClipboardSide::Client, 0);
        }

        if !name.contains("block-input") {
            self.save_config(config);
        }
        let mut misc = Misc::new();
        misc.set_option(option);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        Some(msg_out)
    }

    /// Get [`PeerConfig`] of the current [`LoginConfigHandler`].
    ///
    /// # Arguments
    pub fn get_config(&mut self) -> &mut PeerConfig {
        &mut self.config
    }

    /// Get [`OptionMessage`] of the current [`LoginConfigHandler`].
    /// Return `None` if there's no option, for example, when the session is only for file transfer.
    ///
    /// # Arguments
    ///
    /// * `ignore_default` - If `true`, ignore the default value of the option.
    fn get_option_message(&self, ignore_default: bool) -> Option<OptionMessage> {
        if self.conn_type.eq(&ConnType::PORT_FORWARD)
            || self.conn_type.eq(&ConnType::RDP)
            || self.conn_type.eq(&ConnType::FILE_TRANSFER)
        {
            return None;
        }
        let mut msg = OptionMessage::new();
        if self.conn_type.eq(&ConnType::TERMINAL) {
            if self.get_toggle_option(keys::OPTION_TERMINAL_PERSISTENT) {
                msg.terminal_persistent = BoolOption::Yes.into();
                return Some(msg);
            } else {
                return None;
            }
        }
        let q = self.image_quality.clone();
        if let Some(q) = self.get_image_quality_enum(&q, ignore_default) {
            msg.image_quality = q.into();
        } else if q == "custom" {
            let config = self.load_config();
            let allow_more = true;
            let quality = if config.custom_image_quality.is_empty() {
                50
            } else {
                let mut quality = config.custom_image_quality[0];
                if !allow_more && quality > 100 {
                    quality = 50;
                }
                quality
            };
            msg.custom_image_quality = quality << 8;
            #[cfg(feature = "flutter")]
            if let Some(custom_fps) = self.options.get("custom-fps") {
                let mut custom_fps = custom_fps.parse().unwrap_or(30);
                if !allow_more && custom_fps > 30 {
                    custom_fps = 30;
                }
                msg.custom_fps = custom_fps;
                *self.custom_fps.lock().unwrap() = Some(custom_fps as _);
            }
        }
        let view_only = self.get_toggle_option("view-only");
        if view_only {
            msg.disable_keyboard = BoolOption::Yes.into();
        }
        if view_only || self.get_toggle_option("show-remote-cursor") {
            msg.show_remote_cursor = BoolOption::Yes.into();
        }
        if view_only && self.get_toggle_option("show-my-cursor") {
            msg.show_my_cursor = BoolOption::Yes.into();
        }
        if self.get_toggle_option("follow-remote-cursor") {
            msg.follow_remote_cursor = BoolOption::Yes.into();
        }
        if self.get_toggle_option("follow-remote-window") {
            msg.follow_remote_window = BoolOption::Yes.into();
        }
        if !view_only && self.get_toggle_option("lock-after-session-end") {
            msg.lock_after_session_end = BoolOption::Yes.into();
        }
        if self.get_toggle_option("disable-audio") {
            msg.disable_audio = BoolOption::Yes.into();
        }
        if !view_only && self.get_toggle_option(keys::OPTION_ENABLE_FILE_COPY_PASTE) {
            msg.enable_file_transfer = BoolOption::Yes.into();
        }
        if view_only || self.get_toggle_option("disable-clipboard") {
            msg.disable_clipboard = BoolOption::Yes.into();
        }
        msg.supported_decoding = MessageField::some(self.get_supported_decoding());
        Some(msg)
    }

    pub fn get_supported_decoding(&self) -> SupportedDecoding {
        Decoder::supported_decodings(
            Some(&self.id),
            use_texture_render(),
            self.adapter_luid,
            &self.mark_unsupported,
        )
    }

    /// Parse the image quality option.
    /// Return [`ImageQuality`] if the option is valid, otherwise return `None`.
    ///
    /// # Arguments
    ///
    /// * `q` - The image quality option.
    /// * `ignore_default` - Ignore the default value.
    fn get_image_quality_enum(&self, q: &str, ignore_default: bool) -> Option<ImageQuality> {
        if q == "low" {
            Some(ImageQuality::Low)
        } else if q == "best" {
            Some(ImageQuality::Best)
        } else if q == "balanced" {
            if ignore_default {
                None
            } else {
                Some(ImageQuality::Balanced)
            }
        } else {
            None
        }
    }

    /// Get the status of a toggle option.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the toggle option.
    ///
    // It's Ok to check the option empty in this function.
    // `get_toggle_option()` is only called in a session.
    // Custom client advanced settings will not effect this function.
    pub fn get_toggle_option(&self, name: &str) -> bool {
        if name == "show-remote-cursor" {
            self.config.show_remote_cursor.v
        } else if name == "lock-after-session-end" {
            self.config.lock_after_session_end.v
        } else if name == keys::OPTION_TERMINAL_PERSISTENT {
            self.config.terminal_persistent.v
        } else if name == "privacy-mode" {
            self.config.privacy_mode.v
        } else if name == keys::OPTION_ENABLE_FILE_COPY_PASTE {
            self.config.enable_file_copy_paste.v
        } else if name == "disable-audio" {
            self.config.disable_audio.v
        } else if name == "disable-clipboard" {
            self.config.disable_clipboard.v
        } else if name == "show-quality-monitor" {
            self.config.show_quality_monitor.v
        } else if name == "allow_swap_key" {
            self.config.allow_swap_key.v
        } else if name == "view-only" {
            self.config.view_only.v
        } else if name == "show-my-cursor" {
            self.config.show_my_cursor.v
        } else if name == "follow-remote-cursor" {
            self.config.follow_remote_cursor.v
        } else if name == "follow-remote-window" {
            self.config.follow_remote_window.v
        } else {
            !self.get_option(name).is_empty()
        }
    }

    pub fn is_privacy_mode_supported(&self) -> bool {
        if let Some(features) = &self.features {
            features.privacy_mode
        } else {
            false
        }
    }

    /// Create a [`Message`] for refreshing video.
    pub fn refresh() -> Message {
        let mut misc = Misc::new();
        misc.set_refresh_video(true);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    /// Create a [`Message`] for refreshing video.
    pub fn refresh_display(display: usize) -> Message {
        let mut misc = Misc::new();
        misc.set_refresh_video_display(display as _);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    /// Create a [`Message`] for saving custom image quality.
    ///
    /// # Arguments
    ///
    /// * `bitrate` - The given bitrate.
    /// * `quantizer` - The given quantizer.
    pub fn save_custom_image_quality(&mut self, image_quality: i32) -> Message {
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            custom_image_quality: image_quality << 8,
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        let mut config = self.load_config();
        config.image_quality = "custom".to_owned();
        config.custom_image_quality = vec![image_quality as _];
        self.save_config(config);
        msg_out
    }

    /// Save the given image quality to the config.
    /// Return a [`Message`] that contains image quality, or `None` if the image quality is not valid.
    /// # Arguments
    ///
    /// * `value` - The image quality.
    pub fn save_image_quality(&mut self, value: String) -> Option<Message> {
        let mut res = None;
        if let Some(q) = self.get_image_quality_enum(&value, false) {
            let mut misc = Misc::new();
            misc.set_option(OptionMessage {
                image_quality: q.into(),
                ..Default::default()
            });
            let mut msg_out = Message::new();
            msg_out.set_misc(misc);
            res = Some(msg_out);
        }
        let mut config = self.load_config();
        config.image_quality = value;
        self.save_config(config);
        res
    }

    pub fn save_trackpad_speed(&mut self, speed: i32) {
        let mut config = self.load_config();
        config.trackpad_speed = speed;
        self.save_config(config);
    }

    /// Create a [`Message`] for saving custom fps.
    ///
    /// # Arguments
    ///
    /// * `fps` - The given fps.
    /// * `save_config` - Save the config.
    pub fn set_custom_fps(&mut self, fps: i32, save_config: bool) -> Message {
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            custom_fps: fps,
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        if save_config {
            let mut config = self.load_config();
            config
                .options
                .insert("custom-fps".to_owned(), fps.to_string());
            self.save_config(config);
        }
        *self.custom_fps.lock().unwrap() = Some(fps as _);
        msg_out
    }

    pub fn get_option(&self, k: &str) -> String {
        if let Some(v) = self.config.options.get(k) {
            v.clone()
        } else {
            "".to_owned()
        }
    }

    #[inline]
    pub fn get_custom_resolution(&self, display: i32) -> Option<(i32, i32)> {
        self.config
            .custom_resolutions
            .get(&display.to_string())
            .map(|r| (r.w, r.h))
    }

    #[inline]
    pub fn set_custom_resolution(&mut self, display: i32, wh: Option<(i32, i32)>) {
        let display = display.to_string();
        let mut config = self.load_config();
        match wh {
            Some((w, h)) => {
                config
                    .custom_resolutions
                    .insert(display, Resolution { w, h });
            }
            None => {
                config.custom_resolutions.remove(&display);
            }
        }
        self.save_config(config);
    }

    /// Get user name.
    /// Return the name of the given peer. If the peer has no name, return the name in the config.
    ///
    /// # Arguments
    ///
    /// * `pi` - peer info.
    pub fn get_username(&self, pi: &PeerInfo) -> String {
        return if pi.username.is_empty() {
            self.info.username.clone()
        } else {
            pi.username.clone()
        };
    }

    /// Handle peer info.
    ///
    /// # Arguments
    ///
    /// * `username` - The name of the peer.
    /// * `pi` - The peer info.
    pub fn handle_peer_info(&mut self, pi: &PeerInfo) {
        if !pi.version.is_empty() {
            self.version = hbb_common::get_version_number(&pi.version);
        }
        self.features = pi.features.clone().into_option();
        let serde = PeerInfoSerde {
            username: pi.username.clone(),
            hostname: pi.hostname.clone(),
            platform: pi.platform.clone(),
        };
        let mut config = self.load_config();
        config.info = serde;
        let password = self.password.clone();
        let password0 = config.password.clone();
        let remember = self.remember;
        let hash = self.hash.clone();
        if remember {
            // remember is true: use PeerConfig password or ui login
            // not sync shared password to recent
            // A preset is chosen before the saved password and before the
            // dialog. remember may already be true from that saved password;
            // do not treat it as consent to overwrite. handle_login_from_ui
            // clears the preset source when the user submits Remember.
            if should_replace_saved_password(
                true,
                self.password_source.is_preset(),
                &password,
                &password0,
                self.password_source.is_shared_ab(&password, &hash),
            ) {
                config.password = password.clone();
                log::debug!("remember password of {}", self.id);
            }
        } else {
            if self.password_source.is_personal_ab(&password) {
                // sync personal ab password to recent automatically
                config.password = password.clone();
                log::debug!("save ab password of {} to recent", self.id);
            } else if should_clear_saved_password(self.password_source.is_preset(), &password0) {
                config.password = Default::default();
                log::debug!("remove password of {}", self.id);
            }
        }
        #[cfg(feature = "flutter")]
        {
            // sync connected password to personal ab automatically if it is not shared password
            if !config.password.is_empty()
                && !self.password_source.is_shared_ab(&password, &hash)
                && !self.password_source.is_personal_ab(&password)
            {
                let hash = base64::encode(config.password.clone(), base64::Variant::Original);
                let evt: HashMap<&str, String> = HashMap::from([
                    ("name", "sync_peer_hash_password_to_personal_ab".to_string()),
                    ("id", self.id.clone()),
                    ("hash", hash),
                ]);
                let evt = serde_json::ser::to_string(&evt).unwrap_or("".to_owned());
                crate::flutter::push_global_event(crate::flutter::APP_TYPE_MAIN, evt);
            }
        }
        if config.keyboard_mode.is_empty() {
            if is_keyboard_mode_supported(
                &KeyboardMode::Map,
                get_version_number(&pi.version),
                &pi.platform,
            ) {
                config.keyboard_mode = KeyboardMode::Map.to_string();
            } else {
                config.keyboard_mode = KeyboardMode::Legacy.to_string();
            }
        } else {
            let keyboard_modes =
                crate::get_supported_keyboard_modes(get_version_number(&pi.version), &pi.platform);
            let current_mode = &KeyboardMode::from_str(&config.keyboard_mode).unwrap_or_default();
            if !keyboard_modes.contains(current_mode) {
                config.keyboard_mode = KeyboardMode::Legacy.to_string();
            }
        }
        // no matter if change, for update file time
        self.save_config(config);
        self.supported_encoding = pi.encoding.clone().unwrap_or_default();
        log::info!("peer info supported_encoding:{:?}", self.supported_encoding);
    }

    pub fn get_remote_dir(&self) -> String {
        serde_json::from_str::<HashMap<String, String>>(&self.get_option("remote_dir"))
            .unwrap_or_default()
            .remove(&self.info.username)
            .unwrap_or_default()
    }

    pub fn get_all_remote_dir(&self, path: String) -> String {
        let d = self.get_option("remote_dir");
        let user = self.info.username.clone();
        let mut x = serde_json::from_str::<HashMap<String, String>>(&d).unwrap_or_default();
        if path.is_empty() {
            x.remove(&user);
        } else {
            x.insert(user, path);
        }
        serde_json::to_string::<HashMap<String, String>>(&x).unwrap_or_default()
    }

    /// Create a [`Message`] for login.
    fn create_login_msg(
        &self,
        os_username: String,
        os_password: String,
        password: Vec<u8>,
    ) -> Message {
        let my_id = Config::get_id();
        let (my_id, pure_id) = (my_id, self.id.clone());
        let mut avatar = get_builtin_option(keys::OPTION_AVATAR);
        if avatar.is_empty() {
            avatar = serde_json::from_str::<serde_json::Value>(&LocalConfig::get_option(
                "user_info",
            ))
            .ok()
            .and_then(|x| {
                x.get("avatar")
                    .and_then(|x| x.as_str())
                    .map(|x| x.trim().to_owned())
            })
            .unwrap_or_default();
        }
        avatar = resolve_avatar_url(avatar);
        let mut display_name = get_builtin_option(keys::OPTION_DISPLAY_NAME);
        if display_name.is_empty() {
            display_name =
                serde_json::from_str::<serde_json::Value>(&LocalConfig::get_option("user_info"))
                    .map(|x| {
                        x.get("display_name")
                            .and_then(|x| x.as_str())
                            .map(|x| x.trim())
                            .filter(|x| !x.is_empty())
                            .or_else(|| x.get("name").and_then(|x| x.as_str()))
                            .map(|x| x.to_owned())
                            .unwrap_or_default()
                    })
                    .unwrap_or_default();
        }
        if display_name.is_empty() {
            display_name = crate::username();
        }
        let display_name = display_name
            .split_whitespace()
            .map(|word| {
                word.chars()
                    .enumerate()
                    .map(|(i, c)| {
                        if i == 0 {
                            c.to_uppercase().to_string()
                        } else {
                            c.to_string()
                        }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join(" ");
        #[cfg(not(target_os = "android"))]
        let my_platform = hbb_common::whoami::platform().to_string();
        #[cfg(target_os = "android")]
        let my_platform = "Android".into();
        let hwid = if self.get_option("trust-this-device") == "Y" {
            crate::get_hwid()
        } else {
            Bytes::new()
        };
        let os_login: MessageField<OSLogin> = if self.conn_type == ConnType::TERMINAL {
            Some(OSLogin {
                username: os_username,
                password: os_password,
                ..Default::default()
            })
            .into()
        } else {
            Default::default()
        };
        let mut lr = LoginRequest {
            username: pure_id,
            password: password.into(),
            my_id,
            my_name: display_name,
            my_platform,
            option: self.get_option_message(true).into(),
            session_id: self.session_id,
            version: crate::PROTOCOL_VERSION.to_string(),
            os_login,
            hwid,
            avatar,
            ..Default::default()
        };
        match self.conn_type {
            ConnType::FILE_TRANSFER => lr.set_file_transfer(FileTransfer {
                dir: self.get_remote_dir(),
                show_hidden: !self.get_option("remote_show_hidden").is_empty(),
                ..Default::default()
            }),
            ConnType::VIEW_CAMERA => lr.set_view_camera(Default::default()),
            ConnType::PORT_FORWARD | ConnType::RDP => lr.set_port_forward(PortForward {
                host: self.port_forward.0.clone(),
                port: self.port_forward.1,
                multiplex: self.port_forward_multiplex,
                ..Default::default()
            }),
            ConnType::TERMINAL => {
                let mut terminal = Terminal::new();
                terminal.service_id = self.get_option(self.get_key_terminal_service_id());
                lr.set_terminal(terminal);
            }
            _ => {}
        }

        let mut msg_out = Message::new();
        msg_out.set_login_request(lr);
        msg_out
    }

    pub fn update_supported_decodings(&self) -> Message {
        let decoding = scrap::codec::Decoder::supported_decodings(
            Some(&self.id),
            use_texture_render(),
            self.adapter_luid,
            &self.mark_unsupported,
        );
        let mut misc = Misc::new();
        misc.set_option(OptionMessage {
            supported_decoding: hbb_common::protobuf::MessageField::some(decoding),
            ..Default::default()
        });
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    pub fn restart_remote_device(&self) -> Message {
        let mut misc = Misc::new();
        misc.set_restart_remote_device(true);
        let mut msg_out = Message::new();
        msg_out.set_misc(misc);
        msg_out
    }

    pub fn mark_restarting_remote_device(&mut self) {
        self.restarting_remote_device = true;
        self.restart_remote_device_at = Some(Instant::now());
    }

    pub fn clear_restarting_remote_device(&mut self) {
        self.restarting_remote_device = false;
        self.restart_remote_device_at = None;
    }

    pub fn is_restarting_remote_device(&self) -> bool {
        if !self.restarting_remote_device {
            return false;
        }
        // Keep this flag alive for a short grace window instead of clearing it on
        // connection_ready or the first peer bytes. During OS restart the peer can
        // briefly reconnect before the real reboot disconnect, and clearing it too
        // early would let the next disconnect escape the restart flow and fall back
        // to the normal error dialog / manual reconnect path.
        self.restart_remote_device_at
            .map(|started_at| started_at.elapsed() < RESTART_REMOTE_DEVICE_GRACE)
            .unwrap_or(false)
    }

    pub fn get_conn_token(&self) -> Option<String> {
        if self.password.is_empty() {
            return None;
        }
        serde_json::to_string(&ConnToken {
            password: self.password.clone(),
            password_source: self.password_source.clone(),
            session_id: self.session_id,
        })
        .ok()
    }

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub fn get_key_terminal_service_id(&self) -> &'static str {
        if self.is_terminal_admin {
            "terminal-admin-service-id"
        } else {
            "terminal-service-id"
        }
    }
}

/// Media data.
pub enum MediaData {
    VideoQueue,
    VideoFrame(Box<VideoFrame>),
    AudioFrame(Box<AudioFrame>),
    AudioFormat(AudioFormat),
    Reset,
    RecordScreen(bool),
}

pub type MediaSender = mpsc::Sender<MediaData>;

/// Start video thread.
///
/// # Arguments
///
/// * `video_callback` - The callback for video frame. Being called when a video frame is ready.
pub fn start_video_thread<F, T>(
    session: Session<T>,
    display: usize,
    video_receiver: mpsc::Receiver<MediaData>,
    video_queue: Arc<RwLock<ArrayQueue<VideoFrame>>>,
    fps: Arc<RwLock<Option<usize>>>,
    chroma: Arc<RwLock<Option<Chroma>>>,
    discard_queue: Arc<RwLock<bool>>,
    video_callback: F,
) where
    F: 'static + FnMut(usize, &mut scrap::ImageRgb, *mut c_void, bool) + Send,
    T: InvokeUiSession,
{
    let mut video_callback = video_callback;
    let mut last_chroma = None;
    let is_view_camera = session.is_view_camera();

    std::thread::spawn(move || {
        #[cfg(windows)]
        sync_cpu_usage();
        get_hwcodec_config();
        let mut video_handler = None;
        let mut count = 0;
        let mut duration = std::time::Duration::ZERO;
        let mut skip_beginning = 0;
        loop {
            if let Ok(data) = video_receiver.recv() {
                match data {
                    MediaData::VideoFrame(_) | MediaData::VideoQueue => {
                        let vf = match data {
                            MediaData::VideoFrame(vf) => {
                                *discard_queue.write().unwrap() = false;
                                *vf
                            }
                            MediaData::VideoQueue => {
                                if let Some(vf) = video_queue.read().unwrap().pop() {
                                    if discard_queue.read().unwrap().clone() {
                                        continue;
                                    }
                                    vf
                                } else {
                                    continue;
                                }
                            }
                            _ => {
                                // unreachable!();
                                continue;
                            }
                        };
                        let display = vf.display as usize;
                        let start = std::time::Instant::now();
                        let format = CodecFormat::from(&vf);
                        if video_handler.is_none() {
                            let mut handler = VideoHandler::new(format, display);
                            let record_state = session.lc.read().unwrap().record_state;
                            let record_permission = session.lc.read().unwrap().record_permission;
                            let id = session.lc.read().unwrap().id.clone();
                            if record_state && record_permission {
                                handler.record_screen(true, id, display, is_view_camera);
                            }
                            video_handler = Some(handler);
                        }
                        if let Some(handler) = video_handler.as_mut() {
                            let mut pixelbuffer = true;
                            let mut tmp_chroma = None;
                            let format_changed = handler.decoder.format() != format;
                            match handler.handle_frame(vf, &mut pixelbuffer, &mut tmp_chroma) {
                                Ok(true) => {
                                    video_callback(
                                        display,
                                        &mut handler.rgb,
                                        handler.texture.texture,
                                        pixelbuffer,
                                    );

                                    // chroma
                                    if tmp_chroma.is_some() && last_chroma != tmp_chroma {
                                        last_chroma = tmp_chroma;
                                        *chroma.write().unwrap() = tmp_chroma;
                                    }

                                    // fps calculation
                                    fps_calculate(
                                        &mut skip_beginning,
                                        &fps,
                                        format_changed,
                                        start.elapsed(),
                                        &mut count,
                                        &mut duration,
                                    );
                                }
                                Err(e) => {
                                    // This is a simple workaround.
                                    //
                                    // I only see the following error:
                                    // FailedCall("errcode=1 scrap::common::vpxcodec:libs\\scrap\\src\\common\\vpxcodec.rs:433:9")
                                    // When switching from all displays to one display, the error occurs.
                                    // eg:
                                    // 1. Connect to a device with two displays (A and B).
                                    // 2. Switch to display A. The error occurs.
                                    // 3. If the error does not occur. Switch from A to display B. The error occurs.
                                    //
                                    // to-do: fix the error
                                    log::error!("handle video frame error, {}", e);
                                    session.refresh_video(display as _);
                                }
                                _ => {}
                            }
                        }

                        // check invalid decoders
                        let mut should_update_supported = false;
                        if let Some(handler) = video_handler.as_mut() {
                            if !handler.decoder.valid()
                                || handler.fail_counter >= MAX_DECODE_FAIL_COUNTER
                            {
                                let mut lc = session.lc.write().unwrap();
                                let format = handler.decoder.format();
                                if !lc.mark_unsupported.contains(&format) {
                                    lc.mark_unsupported.push(format);
                                    should_update_supported = true;
                                    log::info!("mark {format:?} decoder as unsupported, valid:{}, fail_counter:{}, all unsupported:{:?}", handler.decoder.valid(), handler.fail_counter, lc.mark_unsupported);
                                }
                            }
                        }
                        if should_update_supported {
                            session.send(Data::Message(
                                session.lc.read().unwrap().update_supported_decodings(),
                            ));
                        }
                    }
                    MediaData::Reset => {
                        if let Some(handler) = video_handler.as_mut() {
                            handler.reset(None);
                        }
                    }
                    MediaData::RecordScreen(start) => {
                        let id = session.lc.read().unwrap().id.clone();
                        if let Some(handler) = video_handler.as_mut() {
                            handler.record_screen(start, id, display, is_view_camera);
                        }
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
        log::info!("Video decoder loop exits");
    });
}

/// Start an audio thread
/// Return a audio [`MediaSender`]
pub fn start_audio_thread() -> MediaSender {
    let (audio_sender, audio_receiver) = mpsc::channel::<MediaData>();
    std::thread::spawn(move || {
        let mut audio_handler = AudioHandler::default();
        loop {
            #[cfg(target_os = "windows")]
            let received = audio_handler.receive_audio(&audio_receiver);
            #[cfg(not(target_os = "windows"))]
            let received = audio_receiver.recv();
            if let Ok(data) = received {
                match data {
                    MediaData::AudioFrame(af) => {
                        audio_handler.handle_frame(*af);
                    }
                    MediaData::AudioFormat(f) => {
                        log::debug!("recved audio format, sample rate={}", f.sample_rate);
                        audio_handler.handle_format(f);
                    }
                    _ => {}
                }
            } else {
                break;
            }
        }
        log::info!("Audio decoder loop exits");
    });
    audio_sender
}

#[inline]
fn fps_calculate(
    skip_beginning: &mut usize,
    fps: &Arc<RwLock<Option<usize>>>,
    format_changed: bool,
    elapsed: std::time::Duration,
    count: &mut usize,
    duration: &mut std::time::Duration,
) {
    if format_changed {
        *count = 0;
        *duration = std::time::Duration::ZERO;
        *skip_beginning = 0;
    }
    // // The first frame will be very slow
    if *skip_beginning < 3 {
        *skip_beginning += 1;
        return;
    }
    *duration += elapsed;
    *count += 1;
    let ms = duration.as_millis();
    if *count % 10 == 0 && ms > 0 {
        *fps.write().unwrap() = Some((*count as usize) * 1000 / (ms as usize));
    }
    // Clear to get real-time fps
    if *count >= 30 {
        *count = 0;
        *duration = Duration::ZERO;
    }
}

fn get_hwcodec_config() {
    #[cfg(feature = "hwcodec")]
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use std::sync::Once;
        static ONCE: Once = Once::new();
        ONCE.call_once(|| {
            let start = std::time::Instant::now();
            if let Err(e) = crate::ipc::get_hwcodec_config_from_server() {
                log::error!(
                    "Failed to get hwcodec config: {e:?}, elapsed: {:?}",
                    start.elapsed()
                );
            } else {
                log::info!("{:?} used to get hwcodec config", start.elapsed());
            }
        });
    }
}

#[cfg(windows)]
fn sync_cpu_usage() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let t = std::thread::spawn(do_sync_cpu_usage);
        t.join().ok();
    });
}

#[cfg(windows)]
#[tokio::main(flavor = "current_thread")]
async fn do_sync_cpu_usage() {
    use crate::ipc::{connect, Data};
    let start = std::time::Instant::now();
    match connect(50, "").await {
        Ok(mut conn) => {
            if conn.send(&&Data::SyncWinCpuUsage(None)).await.is_ok() {
                if let Ok(Some(data)) = conn.next_timeout(50).await {
                    match data {
                        Data::SyncWinCpuUsage(cpu_usage) => {
                            base::platform::windows::sync_cpu_usage(cpu_usage);
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    log::info!("{:?} used to sync cpu usage", start.elapsed());
}

/// Handle latency test.
///
/// # Arguments
///
/// * `t` - The latency test message.
/// * `peer` - The peer.
pub async fn handle_test_delay(t: TestDelay, peer: &mut Stream) {
    if !t.from_client {
        let mut msg_out = Message::new();
        msg_out.set_test_delay(t);
        allow_err!(peer.send(&msg_out).await);
    }
}

/// Whether is track pad scrolling.
#[inline]
#[cfg(all(target_os = "macos", not(feature = "flutter")))]
fn check_scroll_on_mac(mask: i32, x: i32, y: i32) -> bool {
    // flutter version we set mask type bit to 4 when track pad scrolling.
    if mask & 7 == crate::input::MOUSE_TYPE_TRACKPAD {
        return true;
    }
    if mask & 3 != crate::input::MOUSE_TYPE_WHEEL {
        return false;
    }
    let btn = mask >> 3;
    if y == -1 {
        btn != 0xff88 && btn != -0x780000
    } else if y == 1 {
        btn != 0x78 && btn != 0x780000
    } else if x != 0 {
        // No mouse support horizontal scrolling.
        true
    } else {
        false
    }
}

/// Send mouse data.
///
/// # Arguments
///
/// * `mask` - Mouse event.
///     * mask = buttons << 3 | type
///     * type, 1: down, 2: up, 3: wheel, 4: trackpad
///     * buttons, 1: left, 2: right, 4: middle
/// * `x` - X coordinate.
/// * `y` - Y coordinate.
/// * `alt` - Whether the alt key is pressed.
/// * `ctrl` - Whether the ctrl key is pressed.
/// * `shift` - Whether the shift key is pressed.
/// * `command` - Whether the command key is pressed.
/// * `interface` - The interface for sending data.
#[inline]
pub fn send_mouse(
    mask: i32,
    x: i32,
    y: i32,
    alt: bool,
    ctrl: bool,
    shift: bool,
    command: bool,
    interface: &impl Interface,
) {
    let mut msg_out = Message::new();
    let mut mouse_event = MouseEvent {
        mask,
        x,
        y,
        ..Default::default()
    };
    if alt {
        mouse_event.modifiers.push(ControlKey::Alt.into());
    }
    if shift {
        mouse_event.modifiers.push(ControlKey::Shift.into());
    }
    if ctrl {
        mouse_event.modifiers.push(ControlKey::Control.into());
    }
    if command {
        mouse_event.modifiers.push(ControlKey::Meta.into());
    }
    #[cfg(all(target_os = "macos", not(feature = "flutter")))]
    if check_scroll_on_mac(mask, x, y) {
        let factor = 3;
        mouse_event.mask = crate::input::MOUSE_TYPE_TRACKPAD;
        mouse_event.x *= factor;
        mouse_event.y *= factor;
    }
    interface.swap_modifier_mouse(&mut mouse_event);
    msg_out.set_mouse_event(mouse_event);
    interface.send(Data::Message(msg_out));
}

#[inline]
pub fn send_pointer_device_event(
    mut evt: PointerDeviceEvent,
    alt: bool,
    ctrl: bool,
    shift: bool,
    command: bool,
    interface: &impl Interface,
) {
    let mut msg_out = Message::new();
    if alt {
        evt.modifiers.push(ControlKey::Alt.into());
    }
    if shift {
        evt.modifiers.push(ControlKey::Shift.into());
    }
    if ctrl {
        evt.modifiers.push(ControlKey::Control.into());
    }
    if command {
        evt.modifiers.push(ControlKey::Meta.into());
    }
    msg_out.set_pointer_device_event(evt);
    interface.send(Data::Message(msg_out));
}

/// Activate OS by sending mouse movement.
///
/// # Arguments
///
/// * `interface` - The interface for sending data.
/// * `send_left_click` - Whether to send a click event.
fn activate_os(interface: &impl Interface, send_left_click: bool) {
    let left_down = MOUSE_BUTTON_LEFT << 3 | MOUSE_TYPE_DOWN;
    let left_up = MOUSE_BUTTON_LEFT << 3 | MOUSE_TYPE_UP;
    let right_down = MOUSE_BUTTON_RIGHT << 3 | MOUSE_TYPE_DOWN;
    let right_up = MOUSE_BUTTON_RIGHT << 3 | MOUSE_TYPE_UP;
    send_mouse(left_up, 0, 0, false, false, false, false, interface);
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(0, 0, 0, false, false, false, false, interface);
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(0, 3, 3, false, false, false, false, interface);
    let (click_down, click_up) = if send_left_click {
        (left_down, left_up)
    } else {
        (right_down, right_up)
    };
    std::thread::sleep(Duration::from_millis(50));
    send_mouse(click_down, 0, 0, false, false, false, false, interface);
    send_mouse(click_up, 0, 0, false, false, false, false, interface);
    /*
    let mut key_event = KeyEvent::new();
    // do not use Esc, which has problem with Linux
    key_event.set_control_key(ControlKey::RightArrow);
    key_event.press = true;
    let mut msg_out = Message::new();
    msg_out.set_key_event(key_event.clone());
    interface.send(Data::Message(msg_out.clone()));
    */
}

/// Input the OS's password.
///
/// # Arguments
///
/// * `p` - The password.
/// * `activate` - Whether to activate OS.
/// * `interface` - The interface for sending data.
pub fn input_os_password(p: String, activate: bool, interface: impl Interface) {
    std::thread::spawn(move || {
        _input_os_password(p, activate, interface);
    });
}

/// Input the OS's password.
///
/// # Arguments
///
/// * `p` - The password.
/// * `activate` - Whether to activate OS.
/// * `interface` - The interface for sending data.
fn _input_os_password(p: String, activate: bool, interface: impl Interface) {
    let input_password = !p.is_empty();
    if activate {
        // Click event is used to bring up the password input box.
        activate_os(&interface, input_password);
        std::thread::sleep(Duration::from_millis(1200));
    }
    if !input_password {
        return;
    }
    let mut key_event = KeyEvent::new();
    key_event.mode = KeyboardMode::Legacy.into();
    key_event.press = true;
    let mut msg_out = Message::new();
    key_event.set_seq(p);
    msg_out.set_key_event(key_event.clone());
    interface.send(Data::Message(msg_out.clone()));
    key_event.set_control_key(ControlKey::Return);
    msg_out.set_key_event(key_event);
    interface.send(Data::Message(msg_out));
}

/// Auto Login. A peer that can check for its logon or lock screen gets the password and types it
/// itself, only if it is there right then. Keys typed from here land in whatever has focus if the
/// screen is not locked by the time they arrive.
pub fn auto_login(p: String, pi: &PeerInfo, interface: impl Interface) {
    if !is_peer_handling_os_login(pi) {
        input_os_password(p, true, interface);
        return;
    }
    std::thread::spawn(move || {
        activate_os(&interface, true);
        std::thread::sleep(Duration::from_millis(1200));
        let mut msg_out = Message::new();
        msg_out.set_os_login(OSLogin {
            password: p,
            ..Default::default()
        });
        interface.send(Data::Message(msg_out));
    });
}

/// Windows and macOS peers from the 1.5.1 release handle `os_login`.
fn is_peer_handling_os_login(pi: &PeerInfo) -> bool {
    (pi.platform == crate::common::PLATFORM_WINDOWS || pi.platform == crate::common::PLATFORM_MACOS)
        && hbb_common::get_version_number(&pi.version) >= hbb_common::get_version_number("1.5.1")
}

#[derive(Copy, Clone)]
struct LoginErrorMsgBox {
    msgtype: &'static str,
    title: &'static str,
    text: &'static str,
    link: &'static str,
    try_again: bool,
}

lazy_static::lazy_static! {
    static ref LOGIN_ERROR_MAP: Arc<HashMap<&'static str, LoginErrorMsgBox>> = {
        let map = HashMap::from([(LOGIN_SCREEN_WAYLAND, LoginErrorMsgBox{
            msgtype: "error",
            title: "Login Error",
            text: "Login screen using Wayland is not supported",
            link: "",
            try_again: true,
        }), (LOGIN_MSG_NO_PASSWORD_ACCESS, LoginErrorMsgBox{
            msgtype: "wait-remote-accept-nook",
            title: "Prompt",
            text: "Please wait for the remote side to accept your session request...",
            link: "",
            try_again: true,
        })]);
        Arc::new(map)
    };
}

/// Handle login error.
/// Return true if the password is wrong, return false if there's an actual error.
pub fn handle_login_error(
    lc: Arc<RwLock<LoginConfigHandler>>,
    err: &str,
    interface: &impl Interface,
) -> bool {
    if err == LOGIN_MSG_PASSWORD_EMPTY {
        lc.write().unwrap().password = Default::default();
        interface.msgbox("input-password", "Password Required", "", "");
        true
    } else if err == LOGIN_MSG_PASSWORD_WRONG {
        lc.write().unwrap().password = Default::default();
        interface.msgbox("re-input-password", err, "Do you want to enter again?", "");
        true
    } else if err == LOGIN_MSG_2FA_WRONG || err == REQUIRE_2FA {
        let enabled = lc.read().unwrap().get_option("trust-this-device") == "Y";
        if enabled {
            lc.write()
                .unwrap()
                .set_option("trust-this-device".to_string(), "".to_string());
        }
        interface.msgbox("input-2fa", err, "", "");
        true
    } else if LOGIN_ERROR_MAP.contains_key(err) {
        if let Some(msgbox_info) = LOGIN_ERROR_MAP.get(err) {
            interface.msgbox(
                msgbox_info.msgtype,
                msgbox_info.title,
                msgbox_info.text,
                msgbox_info.link,
            );
            msgbox_info.try_again
        } else {
            // unreachable!
            false
        }
    } else {
        if err.contains(SCRAP_X11_REQUIRED) {
            interface.msgbox("error", "Login Error", err, SCRAP_X11_REF_URL);
        } else {
            interface.msgbox("error", "Login Error", err, "");
        }
        false
    }
}

// "Switch sides" requires the incoming-only client to connect back to its
// controlling peer; verify the local pending uuid before opening the connection.
#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn is_switch_sides_back(conn_type: ConnType, interface: &impl Interface) -> bool {
    if conn_type != ConnType::DEFAULT_CONN {
        return false;
    }
    let (id, uuid) = {
        let lch = interface.get_lch();
        let lc = lch.read().unwrap();
        let Some(uuid) = lc.switch_uuid.as_deref() else {
            return false;
        };
        let Ok(uuid) = Uuid::parse_str(uuid) else {
            return false;
        };
        (lc.id.clone(), uuid)
    };
    if !request_local_switch_sides_uuid(
        &id,
        &uuid,
        crate::ipc::SwitchSidesUuidAction::Check,
    )
    .await
    {
        return false;
    }
    let lch = interface.get_lch();
    let lc = lch.read().unwrap();
    let current_uuid = lc
        .switch_uuid
        .as_deref()
        .and_then(|value| Uuid::parse_str(value).ok());
    lc.id == id && current_uuid.as_ref() == Some(&uuid)
}

#[cfg(not(all(feature = "flutter", not(any(target_os = "android", target_os = "ios")))))]
async fn is_switch_sides_back(_conn_type: ConnType, _interface: &impl Interface) -> bool {
    false
}

#[cfg(feature = "flutter")]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn request_local_switch_sides_uuid(
    id: &str,
    uuid: &Uuid,
    action: crate::ipc::SwitchSidesUuidAction,
) -> bool {
    let Ok(mut conn) = crate::ipc::connect(1000, "").await else {
        return false;
    };
    let uuid = uuid.to_string();
    if conn
        .send(&crate::ipc::Data::SwitchSidesUuid(
            uuid.clone(),
            id.to_owned(),
            action,
            None,
        ))
        .await
        .is_err()
    {
        return false;
    }
    match conn.next_timeout(1000).await {
        Ok(Some(crate::ipc::Data::SwitchSidesUuid(
            returned_uuid,
            returned_id,
            returned_action,
            Some(true),
        ))) => {
            returned_uuid == uuid && returned_id == id && returned_action == action
        }
        _ => false,
    }
}

/// Handle hash message sent by peer.
/// Hash will be used for login.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `hash` - Hash sent by peer.
/// * `interface` - [`Interface`] for sending data.
/// * `peer` - [`Stream`] for communicating with peer.
pub async fn handle_hash(
    lc: Arc<RwLock<LoginConfigHandler>>,
    password_preset: &str,
    hash: Hash,
    interface: &impl Interface,
    peer: &mut Stream,
) -> bool {
    lc.write().unwrap().hash = hash.clone();
    // Take care of password application order

    // switch_uuid
    #[cfg(feature = "flutter")]
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let uuid = lc.write().unwrap().switch_uuid.take();
        if let Some(uuid) = uuid {
            if let Ok(uuid) = uuid::Uuid::from_str(&uuid) {
                let id = lc.read().unwrap().id.clone();
                if !request_local_switch_sides_uuid(
                    &id,
                    &uuid,
                    crate::ipc::SwitchSidesUuidAction::Consume,
                )
                .await
                {
                    log::warn!("Ignored untrusted switch_uuid");
                } else {
                    lc.write().unwrap().allow_switch_back_once();
                    send_switch_login_request(lc.clone(), peer, uuid).await;
                    lc.write().unwrap().password_source = Default::default();
                    return true;
                }
            }
        }
        // Incoming-only may connect out solely for a verified switch-back;
        // never fall through to password login, including on repeated hashes.
        if config::is_incoming_only() {
            interface.msgbox("error", "Connection Error", "Incoming only mode", "");
            let mut misc = Misc::new();
            misc.set_close_reason(
                "Connection not allowed in incoming-only mode".to_owned(),
            );
            let mut msg = Message::new();
            msg.set_misc(misc);
            allow_err!(peer.send(&msg).await);
            return false;
        }
    }
    // last password
    let mut password = lc.read().unwrap().password.clone();
    // preset password
    if password.is_empty() {
        if !password_preset.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(password_preset);
            hasher.update(&hash.salt);
            let res = hasher.finalize();
            password = res[..].into();
            // Not a user "remember" choice. handle_peer_info must not
            // overwrite a saved password with this hash.
            lc.write().unwrap().password_source = PasswordSource::Preset;
        }
    }
    // shared password
    // Currently it's used only when click shared ab peer card
    let shared_password = lc.write().unwrap().shared_password.take();
    if let Some(shared_password) = shared_password {
        if !shared_password.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(shared_password.clone());
            hasher.update(&hash.salt);
            let res = hasher.finalize();
            password = res[..].into();
            lc.write().unwrap().password_source = PasswordSource::SharedAb(shared_password);
        }
    }
    // peer config password
    if password.is_empty() {
        password = lc.read().unwrap().config.password.clone();
        if !password.is_empty() {
            lc.write().unwrap().password_source = Default::default();
        }
    }
    // personal ab password
    if password.is_empty() {
        try_get_password_from_personal_ab(lc.clone(), &mut password);
    }

    if password.is_empty() {
        let p = crate::ui_interface::get_builtin_option(keys::OPTION_DEFAULT_CONNECT_PASSWORD);
        if !p.is_empty() {
            let mut hasher = Sha256::new();
            hasher.update(p.clone());
            hasher.update(&hash.salt);
            let res = hasher.finalize();
            password = res[..].into();
            lc.write().unwrap().password_source = PasswordSource::SharedAb(p); // reuse SharedAb here
        }
    }

    lc.write().unwrap().password = password.clone();

    let is_terminal_admin = lc.read().unwrap().is_terminal_admin;
    let is_terminal = lc.read().unwrap().conn_type.eq(&ConnType::TERMINAL);
    if is_terminal && is_terminal_admin {
        if password.is_empty() {
            interface.msgbox("terminal-admin-login-password", "", "", "");
        } else {
            interface.msgbox("terminal-admin-login", "", "", "");
        }
        lc.write().unwrap().hash = hash;
        return true;
    }

    let password = if password.is_empty() {
        // login without password, the remote side can click accept
        interface.msgbox("input-password", "Password Required", "", "");
        Vec::new()
    } else {
        let mut hasher = Sha256::new();
        hasher.update(&password);
        hasher.update(&hash.challenge);
        hasher.finalize()[..].into()
    };

    send_login(lc.clone(), String::new(), String::new(), password, peer).await;
    lc.write().unwrap().hash = hash;
    true
}

#[inline]
fn try_get_password_from_personal_ab(lc: Arc<RwLock<LoginConfigHandler>>, password: &mut Vec<u8>) {
    let access_token = LocalConfig::get_option("access_token");
    let ab = config::Ab::load();
    if !access_token.is_empty() && access_token == ab.access_token {
        let id = lc.read().unwrap().id.clone();
        if let Some(ab) = ab.ab_entries.iter().find(|a| a.personal()) {
            if let Some(p) = ab
                .peers
                .iter()
                .find_map(|p| if p.id == id { Some(p) } else { None })
            {
                if let Ok(hash_password) = base64::decode(p.hash.clone(), base64::Variant::Original)
                {
                    if !hash_password.is_empty() {
                        *password = hash_password.clone();
                        lc.write().unwrap().password_source =
                            PasswordSource::PersonalAb(hash_password);
                    }
                }
            }
        }
    }
}

/// Send login message to peer.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `os_username` - OS username.
/// * `os_password` - OS password.
/// * `password` - Password.
/// * `peer` - [`Stream`] for communicating with peer.
async fn send_login(
    lc: Arc<RwLock<LoginConfigHandler>>,
    os_username: String,
    os_password: String,
    password: Vec<u8>,
    peer: &mut Stream,
) {
    let msg_out = lc
        .read()
        .unwrap()
        .create_login_msg(os_username, os_password, password);
    allow_err!(peer.send(&msg_out).await);
}

/// Handle login request made from ui.
///
/// # Arguments
///
/// * `lc` - Login config.
/// * `os_username` - OS username.
/// * `os_password` - OS password.
/// * `password` - Password.
/// * `remember` - Whether to remember password.
/// * `peer` - [`Stream`] for communicating with peer.
pub async fn handle_login_from_ui(
    lc: Arc<RwLock<LoginConfigHandler>>,
    os_username: String,
    os_password: String,
    password: String,
    remember: bool,
    peer: &mut Stream,
) {
    let mut hash_password = if password.is_empty() {
        let mut password2 = lc.read().unwrap().password.clone();
        if password2.is_empty() {
            password2 = lc.read().unwrap().config.password.clone();
            if !password2.is_empty() {
                lc.write().unwrap().password_source = Default::default();
            }
        }
        password2
    } else {
        lc.write().unwrap().password_source = Default::default();
        let mut hasher = Sha256::new();
        hasher.update(password);
        hasher.update(&lc.read().unwrap().hash.salt);
        let res = hasher.finalize();
        lc.write().unwrap().remember = remember;
        res[..].into()
    };
    lc.write().unwrap().password = hash_password.clone();
    let mut hasher2 = Sha256::new();
    hasher2.update(&hash_password[..]);
    hasher2.update(&lc.read().unwrap().hash.challenge);
    hash_password = hasher2.finalize()[..].to_vec();

    send_login(lc.clone(), os_username, os_password, hash_password, peer).await;
}

async fn send_switch_login_request(
    lc: Arc<RwLock<LoginConfigHandler>>,
    peer: &mut Stream,
    uuid: Uuid,
) {
    let mut msg_out = Message::new();
    msg_out.set_switch_sides_response(SwitchSidesResponse {
        uuid: Bytes::from(uuid.as_bytes().to_vec()),
        lr: hbb_common::protobuf::MessageField::some(
            lc.read()
                .unwrap()
                .create_login_msg("".to_owned(), "".to_owned(), vec![])
                .login_request()
                .to_owned(),
        ),
        ..Default::default()
    });
    allow_err!(peer.send(&msg_out).await);
}

/// Interface for client to send data and commands.
#[async_trait]
pub trait Interface: Send + Clone + 'static + Sized {
    /// Send message data to remote peer.
    fn send(&self, data: Data);
    fn msgbox(&self, msgtype: &str, title: &str, text: &str, link: &str);
    fn handle_login_error(&self, err: &str) -> bool;
    fn handle_peer_info(&self, pi: PeerInfo);
    fn set_multiple_windows_session(&self, sessions: Vec<WindowsSession>);
    fn on_error(&self, err: &str) {
        self.msgbox("error", "Error", err, "");
    }
    async fn handle_hash(&self, pass: &str, hash: Hash, peer: &mut Stream) -> bool;
    async fn handle_login_from_ui(
        &self,
        os_username: String,
        os_password: String,
        password: String,
        remember: bool,
        peer: &mut Stream,
    );
    async fn handle_test_delay(&self, t: TestDelay, peer: &mut Stream);

    fn get_lch(&self) -> Arc<RwLock<LoginConfigHandler>>;

    fn get_id(&self) -> String {
        self.get_lch().read().unwrap().id.clone()
    }

    fn get_switch_code(&self) -> String {
        match self.get_lch().read().unwrap().switch_uuid.clone() {
            Some(u) if !u.is_empty() => {
                use hbb_common::sodiumoxide::crypto::hash::sha256;
                crate::encode64(sha256::hash(u.as_bytes()).0)
            }
            _ => String::new(),
        }
    }

    fn swap_modifier_mouse(&self, _msg: &mut base::protos::message::MouseEvent) {}

    fn update_received(&self, received: bool) {
        self.get_lch().write().unwrap().received = received;
    }

    fn on_establish_connection_error(&self, err: String) {
        let title = "Connection Error";
        let text = err.to_string();
        let lch = self.get_lch();
        let (is_restarting, received) = {
            let lc = lch.read().unwrap();
            (lc.is_restarting_remote_device(), lc.received)
        };
        if is_restarting {
            log::info!("Restart remote device, suppress connection error: {err}");
            // Flutter treats this as a reconnect control event. The text is kept
            // for legacy UI and existing translation reuse.
            self.msgbox("restarting", "Restarting remote device", "Connection in progress. Please wait.", "");
            return;
        }

        let errno = errno::errno().0;
        log::error!("Connection closed: {err}({errno})");
        self.msgbox("error", title, &text, "");
    }
}

/// Data used by the client interface.
#[derive(Clone)]
pub enum Data {
    Close,
    RejectInsecureConnection,
    Login((String, String, String, bool)),
    Message(Message),
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    InitialClipboard(usize, Option<Message>),
    SendFiles((i32, JobType, String, String, i32, bool, bool)),
    RemoveDirAll((i32, String, bool, bool)),
    ConfirmDeleteFiles((i32, i32)),
    SetNoConfirm(i32),
    RemoveDir((i32, String)),
    RemoveFile((i32, String, i32, bool)),
    CreateDir((i32, String, bool)),
    CancelJob(i32),
    RemovePortForward(i32),
    AddPortForward((i32, String, i32)),
    #[cfg(all(target_os = "windows", not(feature = "flutter")))]
    ToggleClipboardFile,
    NewRDP,
    SetConfirmOverrideFile((i32, i32, bool, bool, bool)),
    AddJob((i32, JobType, String, String, i32, bool, bool)),
    ResumeJob((i32, bool)),
    RecordScreen(bool),
    ElevateDirect,
    ElevateWithLogon(String, String),
    NewVoiceCall,
    CloseVoiceCall,
    ContinueInsecureConnection,
    ResetDecoder(Option<usize>),
    RenameFile((i32, String, String, bool)),
    TakeScreenshot((i32, String)),
}

pub async fn confirm_insecure_connection(
    interface: &impl Interface,
    receiver: &mut UnboundedReceiver<Data>,
    direct_host_key: Option<&str>,
) -> bool {
    let id = interface.get_id();
    let prompt =
        direct_host_key.and_then(|key| crate::lan::direct_confirm_prompt(&id, key));
    if let Some(prompt) = prompt.as_ref() {
        interface.msgbox(prompt.msg_type, prompt.title, &prompt.text, "");
    } else {
        let typed_direct = crate::lan::is_typed_direct(&id);
        interface.msgbox(
            "insecure-connection-nocancel-hasclose",
            if typed_direct {
                "Unverified identity"
            } else {
                "Insecure Connection"
            },
            if typed_direct {
                "Unverified identity"
            } else if hbb_common::is_ip_str(&id) {
                "Direct and unencrypted connection"
            } else {
                "conn-e2ee-unavailable-tip"
            },
            "",
        );
    }
    while let Some(data) = receiver.recv().await {
        match data {
            Data::ContinueInsecureConnection => {
                if let Some(key) = direct_host_key {
                    if prompt.is_some() {
                        crate::lan::accept_direct_host(&id, key);
                    }
                }
                return true;
            }
            Data::RejectInsecureConnection => return false,
            Data::Close => return false,
            _ => {}
        }
    }
    false
}

/// Keycode for key events.
#[derive(Clone, Debug)]
pub enum Key {
    ControlKey(ControlKey),
    Chr(u32),
    _Raw(u32),
}

lazy_static::lazy_static! {
    pub static ref KEY_MAP: HashMap<&'static str, Key> =
    [
        ("VK_A", Key::Chr('a' as _)),
        ("VK_B", Key::Chr('b' as _)),
        ("VK_C", Key::Chr('c' as _)),
        ("VK_D", Key::Chr('d' as _)),
        ("VK_E", Key::Chr('e' as _)),
        ("VK_F", Key::Chr('f' as _)),
        ("VK_G", Key::Chr('g' as _)),
        ("VK_H", Key::Chr('h' as _)),
        ("VK_I", Key::Chr('i' as _)),
        ("VK_J", Key::Chr('j' as _)),
        ("VK_K", Key::Chr('k' as _)),
        ("VK_L", Key::Chr('l' as _)),
        ("VK_M", Key::Chr('m' as _)),
        ("VK_N", Key::Chr('n' as _)),
        ("VK_O", Key::Chr('o' as _)),
        ("VK_P", Key::Chr('p' as _)),
        ("VK_Q", Key::Chr('q' as _)),
        ("VK_R", Key::Chr('r' as _)),
        ("VK_S", Key::Chr('s' as _)),
        ("VK_T", Key::Chr('t' as _)),
        ("VK_U", Key::Chr('u' as _)),
        ("VK_V", Key::Chr('v' as _)),
        ("VK_W", Key::Chr('w' as _)),
        ("VK_X", Key::Chr('x' as _)),
        ("VK_Y", Key::Chr('y' as _)),
        ("VK_Z", Key::Chr('z' as _)),
        ("VK_0", Key::Chr('0' as _)),
        ("VK_1", Key::Chr('1' as _)),
        ("VK_2", Key::Chr('2' as _)),
        ("VK_3", Key::Chr('3' as _)),
        ("VK_4", Key::Chr('4' as _)),
        ("VK_5", Key::Chr('5' as _)),
        ("VK_6", Key::Chr('6' as _)),
        ("VK_7", Key::Chr('7' as _)),
        ("VK_8", Key::Chr('8' as _)),
        ("VK_9", Key::Chr('9' as _)),
        ("VK_COMMA", Key::Chr(',' as _)),
        ("VK_SLASH", Key::Chr('/' as _)),
        ("VK_SEMICOLON", Key::Chr(';' as _)),
        ("VK_QUOTE", Key::Chr('\'' as _)),
        ("VK_LBRACKET", Key::Chr('[' as _)),
        ("VK_RBRACKET", Key::Chr(']' as _)),
        ("VK_BACKSLASH", Key::Chr('\\' as _)),
        ("VK_MINUS", Key::Chr('-' as _)),
        ("VK_PLUS", Key::Chr('=' as _)),
        ("VK_DIVIDE", Key::ControlKey(ControlKey::Divide)), // numpad
        ("VK_MULTIPLY", Key::ControlKey(ControlKey::Multiply)), // numpad
        ("VK_SUBTRACT", Key::ControlKey(ControlKey::Subtract)), // numpad
        ("VK_ADD", Key::ControlKey(ControlKey::Add)), // numpad
        ("VK_DECIMAL", Key::ControlKey(ControlKey::Decimal)), // numpad
        ("VK_F1", Key::ControlKey(ControlKey::F1)),
        ("VK_F2", Key::ControlKey(ControlKey::F2)),
        ("VK_F3", Key::ControlKey(ControlKey::F3)),
        ("VK_F4", Key::ControlKey(ControlKey::F4)),
        ("VK_F5", Key::ControlKey(ControlKey::F5)),
        ("VK_F6", Key::ControlKey(ControlKey::F6)),
        ("VK_F7", Key::ControlKey(ControlKey::F7)),
        ("VK_F8", Key::ControlKey(ControlKey::F8)),
        ("VK_F9", Key::ControlKey(ControlKey::F9)),
        ("VK_F10", Key::ControlKey(ControlKey::F10)),
        ("VK_F11", Key::ControlKey(ControlKey::F11)),
        ("VK_F12", Key::ControlKey(ControlKey::F12)),
        ("VK_ENTER", Key::ControlKey(ControlKey::Return)),
        ("VK_CANCEL", Key::ControlKey(ControlKey::Cancel)),
        ("VK_BACK", Key::ControlKey(ControlKey::Backspace)),
        ("VK_TAB", Key::ControlKey(ControlKey::Tab)),
        ("VK_CLEAR", Key::ControlKey(ControlKey::Clear)),
        ("VK_RETURN", Key::ControlKey(ControlKey::Return)),
        ("VK_SHIFT", Key::ControlKey(ControlKey::Shift)),
        ("VK_CONTROL", Key::ControlKey(ControlKey::Control)),
        ("VK_MENU", Key::ControlKey(ControlKey::Alt)),
        ("VK_PAUSE", Key::ControlKey(ControlKey::Pause)),
        ("VK_CAPITAL", Key::ControlKey(ControlKey::CapsLock)),
        ("VK_KANA", Key::ControlKey(ControlKey::Kana)),
        ("VK_HANGUL", Key::ControlKey(ControlKey::Hangul)),
        ("VK_JUNJA", Key::ControlKey(ControlKey::Junja)),
        ("VK_FINAL", Key::ControlKey(ControlKey::Final)),
        ("VK_HANJA", Key::ControlKey(ControlKey::Hanja)),
        ("VK_KANJI", Key::ControlKey(ControlKey::Kanji)),
        ("VK_ESCAPE", Key::ControlKey(ControlKey::Escape)),
        ("VK_CONVERT", Key::ControlKey(ControlKey::Convert)),
        ("VK_SPACE", Key::ControlKey(ControlKey::Space)),
        ("VK_PRIOR", Key::ControlKey(ControlKey::PageUp)),
        ("VK_NEXT", Key::ControlKey(ControlKey::PageDown)),
        ("VK_END", Key::ControlKey(ControlKey::End)),
        ("VK_HOME", Key::ControlKey(ControlKey::Home)),
        ("VK_LEFT", Key::ControlKey(ControlKey::LeftArrow)),
        ("VK_UP", Key::ControlKey(ControlKey::UpArrow)),
        ("VK_RIGHT", Key::ControlKey(ControlKey::RightArrow)),
        ("VK_DOWN", Key::ControlKey(ControlKey::DownArrow)),
        ("VK_SELECT", Key::ControlKey(ControlKey::Select)),
        ("VK_PRINT", Key::ControlKey(ControlKey::Print)),
        ("VK_EXECUTE", Key::ControlKey(ControlKey::Execute)),
        ("VK_SNAPSHOT", Key::ControlKey(ControlKey::Snapshot)),
        ("VK_SCROLL", Key::ControlKey(ControlKey::Scroll)),
        ("VK_INSERT", Key::ControlKey(ControlKey::Insert)),
        ("VK_DELETE", Key::ControlKey(ControlKey::Delete)),
        ("VK_HELP", Key::ControlKey(ControlKey::Help)),
        ("VK_SLEEP", Key::ControlKey(ControlKey::Sleep)),
        ("VK_SEPARATOR", Key::ControlKey(ControlKey::Separator)),
        ("VK_NUMPAD0", Key::ControlKey(ControlKey::Numpad0)),
        ("VK_NUMPAD1", Key::ControlKey(ControlKey::Numpad1)),
        ("VK_NUMPAD2", Key::ControlKey(ControlKey::Numpad2)),
        ("VK_NUMPAD3", Key::ControlKey(ControlKey::Numpad3)),
        ("VK_NUMPAD4", Key::ControlKey(ControlKey::Numpad4)),
        ("VK_NUMPAD5", Key::ControlKey(ControlKey::Numpad5)),
        ("VK_NUMPAD6", Key::ControlKey(ControlKey::Numpad6)),
        ("VK_NUMPAD7", Key::ControlKey(ControlKey::Numpad7)),
        ("VK_NUMPAD8", Key::ControlKey(ControlKey::Numpad8)),
        ("VK_NUMPAD9", Key::ControlKey(ControlKey::Numpad9)),
        ("Apps", Key::ControlKey(ControlKey::Apps)),
        ("Meta", Key::ControlKey(ControlKey::Meta)),
        ("RAlt", Key::ControlKey(ControlKey::RAlt)),
        ("RWin", Key::ControlKey(ControlKey::RWin)),
        ("RControl", Key::ControlKey(ControlKey::RControl)),
        ("RShift", Key::ControlKey(ControlKey::RShift)),
        ("CTRL_ALT_DEL", Key::ControlKey(ControlKey::CtrlAltDel)),
        ("LOCK_SCREEN", Key::ControlKey(ControlKey::LockScreen)),
    ].iter().cloned().collect();
}

/// Check if the given message is an error and can be retried.
///
/// # Arguments
///
/// * `msgtype` - The message type.
/// * `title` - The title of the message.
/// * `text` - The text of the message.
#[inline]
pub fn check_if_retry(msgtype: &str, title: &str, text: &str) -> bool {
    msgtype == "error"
        && title == "Connection Error"
        && (!text.to_lowercase().contains("offline")
            && !text.to_lowercase().contains("not exist")
            && !text.to_lowercase().contains("handshake")
            && !text.to_lowercase().contains("failed")
            && !text.to_lowercase().contains("resolve")
            && !text.to_lowercase().contains("mismatch")
            && !text.to_lowercase().contains("manually")
            && !text.to_lowercase().contains("authorization revoked")
            && !text.to_lowercase().contains("restricted")
            && !text.to_lowercase().contains("incoming only")
            && !text.to_lowercase().contains("not allowed"))
}

/// Saved peer password is replaced only when the user asked to remember a
/// different, non-shared password. A connect/pairing preset is not that ask:
/// `remember` is initialized from the existing saved password.
fn should_replace_saved_password(
    remember: bool,
    password_from_preset: bool,
    password: &[u8],
    saved: &[u8],
    shared_ab: bool,
) -> bool {
    remember && !password_from_preset && !password.is_empty() && password != saved && !shared_ab
}

/// A preset login must not delete a saved password. Clearing still happens
/// when the user submits the dialog with Remember unchecked (source is no
/// longer preset).
fn should_clear_saved_password(password_from_preset: bool, saved: &[u8]) -> bool {
    !password_from_preset && !saved.is_empty()
}

#[cfg(test)]
mod retry_tests {
    use super::check_if_retry;

    #[test]
    fn lan_revocation_is_not_retryable() {
        assert!(!check_if_retry("error", "Connection Error", "LAN authorization revoked"));
    }
    #[test]
    fn incoming_only_error_is_not_retryable() {
        assert!(!check_if_retry(
            "error",
            "Connection Error",
            "Incoming only mode",
        ));
    }
}

#[cfg(test)]
mod password_remember_tests {
    use super::{
        should_clear_saved_password, should_replace_saved_password, ConnToken, LoginConfigHandler,
        PasswordSource,
    };

    #[test]
    fn preset_does_not_replace_or_clear_a_saved_password() {
        let saved = b"permanent-hash";
        let preset = b"temporary-hash";
        assert!(!should_replace_saved_password(
            true, true, preset, saved, false
        ));
        assert!(!should_clear_saved_password(true, saved));
        // User submitted the dialog with Remember checked: source is not preset.
        assert!(should_replace_saved_password(
            true, false, preset, saved, false
        ));
        // User submitted with Remember unchecked.
        assert!(should_clear_saved_password(false, saved));
        // Same hash, or a shared address-book password, is left alone.
        assert!(!should_replace_saved_password(
            true, false, saved, saved, false
        ));
        assert!(!should_replace_saved_password(
            true, false, preset, saved, true
        ));
        assert!(!should_clear_saved_password(false, b""));
    }

    #[test]
    fn preset_source_survives_reconnect_token() {
        let mut lc = LoginConfigHandler::default();
        lc.password = b"temporary-hash".to_vec();
        lc.password_source = PasswordSource::Preset;
        lc.session_id = 7;
        let token = lc.get_conn_token().expect("token");
        let parsed: ConnToken = serde_json::from_str(&token).unwrap();
        assert_eq!(parsed.password_source, PasswordSource::Preset);
        assert!(parsed.password_source.is_preset());
    }
}

