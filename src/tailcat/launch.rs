use super::pairing::PairingCode;
use hbb_common::{anyhow::anyhow, bail, ResultType};
use std::io::{BufRead, Read};
#[cfg(any(feature = "flutter", test))]
use std::sync::Mutex;

pub(crate) const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const CONTROL_CODE_ENV: &str = "SEALDESK_CONTROL_CODE";
const MAX_CONTROL_TEXT_BYTES: u64 = 16 * 1024;
const CONTROL_USAGE: &str = "用法：--host，或 --control [不含密码的配对码 | 二维码图片 | -]，或 --control-file <配对码文件>（仅 Linux 与 macOS）；含密码的配对码请从标准输入、限制权限的文件或环境变量 SEALDESK_CONTROL_CODE 读取。Windows 不支持 --control-file，请改用 --control - 或 SEALDESK_CONTROL_CODE。--host 不能与 --no-server 同用";
const INLINE_SECRET_ERROR: &str = "配对码含有密码，不能写在命令行参数里。请改用 --control - 从标准输入读取，或环境变量 SEALDESK_CONTROL_CODE。在 Linux 与 macOS 上也可以使用 --control-file <模式为 0600 且属于当前用户的文件>。Windows 不支持 --control-file";
#[cfg_attr(unix, allow(dead_code))]
pub(crate) const CONTROL_FILE_WINDOWS_ERROR: &str = "--control-file 在 Windows 上不可用：无法在这里校验文件所有者和 ACL。请改用 --control - 从标准输入读取，或环境变量 SEALDESK_CONTROL_CODE";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    Host,
    Control,
}

impl Mode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Control => "control",
        }
    }
}

/// Where a control input was read. Stdin, file, and env values are not on the
/// UI command line; Linux and macOS keep the original process argv.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlInputSource {
    Inline,
    Stdin,
    File,
    Env,
}

pub(crate) struct Launch {
    pub(crate) mode: Mode,
    pub(crate) input: Option<String>,
    #[cfg_attr(not(feature = "flutter"), allow(dead_code))]
    pub(crate) control_source: Option<ControlInputSource>,
}

/// What the UI process is allowed to see. `staged_env` is copied into
/// `SEALDESK_CONTROL_CODE` before the UI starts. `argv` is the rewritten
/// argument list and never contains a password-bearing code.
#[cfg(any(feature = "flutter", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ControlUiHandoff {
    pub argv: Vec<String>,
    pub staged_env: Option<String>,
    /// Linux and macOS ignore `StartUi` and keep the process argv. When this
    /// is set, those runners replace that argv with `argv` before the UI starts.
    pub replace_process_argv: bool,
}

/// Staged env wins over every argv form. Stdin, file, and env launches stage
/// the code because the process argv is only `--control -`, `--control-file`,
/// or `--control`. Inline password-less text and QR paths stay on argv.
#[cfg(any(feature = "flutter", test))]
pub(crate) fn control_ui_handoff(input: &str, source: ControlInputSource) -> ControlUiHandoff {
    let has_password = pairing_declares_password(input);
    let out_of_band = has_password
        || matches!(
            source,
            ControlInputSource::Stdin | ControlInputSource::File | ControlInputSource::Env
        );
    if out_of_band {
        ControlUiHandoff {
            argv: vec!["--control".to_owned()],
            staged_env: Some(input.to_owned()),
            replace_process_argv: true,
        }
    } else {
        ControlUiHandoff {
            argv: vec!["--control".to_owned(), input.to_owned()],
            staged_env: None,
            replace_process_argv: false,
        }
    }
}

#[cfg(any(feature = "flutter", test))]
static UI_ARGV_REPLACEMENT: Mutex<bool> = Mutex::new(false);
#[cfg(any(feature = "flutter", test))]
static CONTROL_ARGV_MARKER: &[u8] = b"--control\0";

#[cfg(any(feature = "flutter", test))]
pub(crate) fn stage_control_for_ui(input: &str, source: ControlInputSource) {
    let handoff = control_ui_handoff(input, source);
    match handoff.staged_env {
        Some(value) => std::env::set_var(CONTROL_CODE_ENV, value),
        None => std::env::remove_var(CONTROL_CODE_ENV),
    }
    *UI_ARGV_REPLACEMENT.lock().unwrap() = handoff.replace_process_argv;
}

#[cfg(any(feature = "flutter", test))]
pub(crate) fn take_ui_argv_replacement() -> bool {
    let mut replace = UI_ARGV_REPLACEMENT.lock().unwrap();
    let value = *replace;
    *replace = false;
    value
}

/// Overwrites process arguments the UI will see with `--control` and zeroes
/// the previous argument bytes, so a file path or marker value does not remain
/// on the command line. The code itself is only in `SEALDESK_CONTROL_CODE`.
#[cfg(any(feature = "flutter", test))]
pub(crate) fn apply_control_marker_argv(
    argc: *mut std::os::raw::c_int,
    argv: *mut *mut std::os::raw::c_char,
) {
    if argc.is_null() || argv.is_null() {
        return;
    }
    unsafe {
        let count = *argc;
        if count < 2 {
            return;
        }
        let marker = b"--control";
        let first = *argv.add(1);
        if first.is_null() {
            return;
        }
        let available = hbb_common::libc::strlen(first);
        if available >= marker.len() {
            std::ptr::copy_nonoverlapping(marker.as_ptr(), first.cast::<u8>(), marker.len());
            *first.add(marker.len()) = 0;
            for index in marker.len() + 1..available {
                *first.add(index) = 0;
            }
        } else {
            for index in 0..available {
                *first.add(index) = 0;
            }
            *argv.add(1) = CONTROL_ARGV_MARKER.as_ptr().cast_mut().cast();
        }
        for index in 2..count {
            let arg = *argv.add(index as usize);
            if arg.is_null() {
                continue;
            }
            let len = hbb_common::libc::strlen(arg);
            for byte in 0..len {
                *arg.add(byte) = 0;
            }
        }
        *argv.add(2) = std::ptr::null_mut();
        *argc = 2;
    }
}

#[cfg(all(target_os = "macos", feature = "flutter"))]
pub(crate) fn apply_staged_process_argv() {
    if !take_ui_argv_replacement() {
        return;
    }
    extern "C" {
        fn _NSGetArgc() -> *mut std::os::raw::c_int;
        fn _NSGetArgv() -> *mut *mut *mut std::os::raw::c_char;
    }
    unsafe {
        let argc = _NSGetArgc();
        let argv = _NSGetArgv();
        if argc.is_null() || argv.is_null() || (*argv).is_null() {
            return;
        }
        apply_control_marker_argv(argc, *argv);
    }
}

const NON_FLUTTER_PASSWORD_ERROR: &str = "这个构建没有可以接收含密码配对码的界面，因此不会启动连接。请使用 Flutter 版本；密码通过标准输入、SEALDESK_CONTROL_CODE 或（Linux 与 macOS）--control-file 交给界面，不会放进命令行参数。";

/// Password-bearing codes need a UI that reads `SEALDESK_CONTROL_CODE`.
/// Builds without Flutter have no such UI, and `StartUi` arguments are not
/// applied on Linux or macOS, so continuing would drop the password.
pub(crate) struct NonFlutterConnect {
    pub address: String,
    pub name: Option<String>,
}

pub(crate) fn non_flutter_connect(input: &str) -> ResultType<NonFlutterConnect> {
    let pairing = PairingCode::parse(input).map_err(|err| anyhow!(err))?;
    if pairing.password.is_some() {
        bail!(NON_FLUTTER_PASSWORD_ERROR);
    }
    Ok(NonFlutterConnect {
        address: pairing.address,
        name: pairing.name,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControlFileStat {
    pub is_symlink: bool,
    pub is_file: bool,
    pub uid: u32,
    pub euid: u32,
    pub mode: u32,
}

/// `None` when a regular file may be read. Group and other permission bits are rejected.
pub(crate) fn control_file_stat_error(stat: ControlFileStat) -> Option<&'static str> {
    if stat.is_symlink {
        return Some("配对码文件不能是符号链接");
    }
    if !stat.is_file {
        return Some("配对码文件必须是普通文件");
    }
    if stat.uid != stat.euid {
        return Some("配对码文件必须属于当前用户");
    }
    if stat.mode & 0o077 != 0 {
        return Some("配对码文件权限过宽，请改为 0600 或更严格，且不能给组或其他用户权限");
    }
    None
}

/// Reads and clears `SEALDESK_CONTROL_CODE` so later child processes do not inherit it.
pub(crate) fn take_control_code_env() -> Option<String> {
    let value = std::env::var(CONTROL_CODE_ENV).ok();
    std::env::remove_var(CONTROL_CODE_ENV);
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// Arguments for the Flutter UI. A password-bearing pairing code is never included.
#[cfg(any(feature = "flutter", test))]
pub(crate) fn flutter_control_arguments(input: &str) -> Vec<String> {
    control_ui_handoff(input, ControlInputSource::Inline).argv
}

fn pairing_prefix(input: &str) -> bool {
    input
        .trim()
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SEALDESK:"))
}

/// Agrees with `PairingCode::parse`: only a code that parses and carries a
/// password field counts. A `P` field that is missing its closing `;`, or a
/// code that never reaches the terminating `;;`, is not a password code.
fn pairing_declares_password(input: &str) -> bool {
    PairingCode::parse(input)
        .ok()
        .and_then(|code| code.password)
        .is_some()
}

fn accept_control_text(text: String) -> ResultType<String> {
    let text = text.trim().to_owned();
    if text.is_empty() {
        bail!("配对码是空的");
    }
    PairingCode::parse(&text).map_err(|err| anyhow!(err))?;
    Ok(text)
}

pub(crate) fn read_control_line<R: BufRead>(reader: &mut R) -> ResultType<String> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_CONTROL_TEXT_BYTES + 1)
        .read_until(b'\n', &mut bytes)?;
    if bytes.is_empty() {
        bail!("标准输入没有配对码");
    }
    if bytes.len() as u64 > MAX_CONTROL_TEXT_BYTES {
        bail!("配对码过长");
    }
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("配对码不是有效的 UTF-8 文本"))?;
    accept_control_text(text)
}

#[cfg(unix)]
fn current_euid() -> u32 {
    unsafe { hbb_common::libc::geteuid() }
}

#[cfg(unix)]
fn fstat_control(fd: std::os::unix::io::RawFd) -> ResultType<(ControlFileStat, u64)> {
    let mut stat = unsafe { std::mem::zeroed::<hbb_common::libc::stat>() };
    if unsafe { hbb_common::libc::fstat(fd, &mut stat) } != 0 {
        bail!("无法读取配对码文件，请检查路径和权限");
    }
    let file_type = stat.st_mode & hbb_common::libc::S_IFMT;
    if stat.st_size < 0 {
        bail!("配对码文件必须是非空文本，且不能超过 16 KB");
    }
    Ok((
        ControlFileStat {
            is_symlink: file_type == hbb_common::libc::S_IFLNK,
            is_file: file_type == hbb_common::libc::S_IFREG,
            uid: stat.st_uid,
            euid: current_euid(),
            mode: stat.st_mode & 0o777,
        },
        stat.st_size as u64,
    ))
}

#[cfg(unix)]
fn read_control_bytes(mut file: std::fs::File, len: u64) -> ResultType<String> {
    if len == 0 || len > MAX_CONTROL_TEXT_BYTES {
        bail!("配对码文件必须是非空文本，且不能超过 16 KB");
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_CONTROL_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTROL_TEXT_BYTES {
        bail!("配对码文件必须是非空文本，且不能超过 16 KB");
    }
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("配对码文件不是有效的 UTF-8 文本"))?;
    accept_control_text(text)
}

/// Opens once with `O_NOFOLLOW` and checks `fstat` on that descriptor before
/// reading it. A path stat followed by a second open would accept a different
/// file than the one whose owner and mode were checked.
#[cfg(unix)]
fn read_control_file(path: &str) -> ResultType<String> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(
            hbb_common::libc::O_NOFOLLOW
                | hbb_common::libc::O_CLOEXEC
                | hbb_common::libc::O_NONBLOCK,
        )
        .open(path)
        .map_err(|err| {
            if err.raw_os_error() == Some(hbb_common::libc::ELOOP) {
                anyhow!("配对码文件不能是符号链接")
            } else {
                anyhow!("无法读取配对码文件，请检查路径和权限")
            }
        })?;
    let (inspected, len) = fstat_control(file.as_raw_fd())?;
    if let Some(error) = control_file_stat_error(inspected) {
        bail!(error);
    }
    read_control_bytes(file, len)
}

/// Windows has no owner or DACL check on the opened file that is safe to rely
/// on here. Reject the flag instead of reading whatever path the caller names.
#[cfg(not(unix))]
fn read_control_file(_path: &str) -> ResultType<String> {
    bail!(CONTROL_FILE_WINDOWS_ERROR)
}

impl Launch {
    pub(crate) fn parse(args: &[String], no_server: bool) -> ResultType<Option<Self>> {
        Self::parse_from(args, no_server, None, &mut std::io::Cursor::new([]))
    }

    pub(crate) fn parse_from<R: BufRead>(
        args: &[String],
        no_server: bool,
        env_code: Option<String>,
        stdin: &mut R,
    ) -> ResultType<Option<Self>> {
        let Some(command) = args.first().map(String::as_str) else {
            return Ok(None);
        };
        match command {
            "--host" => {
                if args.len() != 1 || no_server {
                    bail!(CONTROL_USAGE);
                }
                return Ok(Some(Self {
                    mode: Mode::Host,
                    input: None,
                    control_source: None,
                }));
            }
            "--control-file" => {
                if args.len() != 2 {
                    bail!(CONTROL_USAGE);
                }
                let text = read_control_file(&args[1])?;
                return Ok(Some(Self {
                    mode: Mode::Control,
                    input: Some(text),
                    control_source: Some(ControlInputSource::File),
                }));
            }
            "--control" => {}
            _ => return Ok(None),
        }
        if args.len() > 2 {
            bail!(CONTROL_USAGE);
        }
        let Some(input) = args.get(1) else {
            let input = match env_code {
                Some(env_code) => Some(accept_control_text(env_code)?),
                None => None,
            };
            let control_source = input.as_ref().map(|_| ControlInputSource::Env);
            return Ok(Some(Self {
                mode: Mode::Control,
                input,
                control_source,
            }));
        };
        if input == "-" {
            return Ok(Some(Self {
                mode: Mode::Control,
                input: Some(read_control_line(stdin)?),
                control_source: Some(ControlInputSource::Stdin),
            }));
        }
        if input.starts_with("--") || input.is_empty() {
            bail!("--control 需要不含密码的配对码、二维码图片路径，或 - 表示从标准输入读取");
        }
        if pairing_prefix(input) {
            if pairing_declares_password(input) {
                bail!(INLINE_SECRET_ERROR);
            }
            PairingCode::parse(input).map_err(|err| anyhow!(err))?;
            return Ok(Some(Self {
                mode: Mode::Control,
                input: Some(input.clone()),
                control_source: Some(ControlInputSource::Inline),
            }));
        }
        let metadata = std::fs::metadata(input)
            .map_err(|_| anyhow!("无法读取二维码图片，请检查文件路径和权限"))?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
            bail!("二维码图片必须是非空普通文件，且不能超过 20 MB");
        }
        #[cfg(not(feature = "flutter"))]
        bail!("二维码图片命令行导入需要 Flutter 版本；当前版本请使用配对码文本");
        #[cfg(feature = "flutter")]
        Ok(Some(Self {
            mode: Mode::Control,
            input: Some(input.clone()),
            control_source: Some(ControlInputSource::Inline),
        }))
    }
}

/// Explicit blocks only. The desktop UI page is fixed and is not an input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HostServiceGate {
    pub explicit_control: bool,
    pub no_server: bool,
    pub outgoing_only: bool,
    pub stop_service: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StartupServicePlan {
    pub start_server: bool,
    /// `Some("host" | "control")` to apply through `request_mode`.
    /// `None` leaves host mode and stop-service untouched.
    pub request_mode: Option<&'static str>,
}

/// Desktop cold start always shares unless the launch or the stop-service
/// setting says not to. `sealdesk-mode` is not consulted.
pub(crate) fn plan_startup_service(gate: HostServiceGate) -> StartupServicePlan {
    if gate.explicit_control || gate.no_server || gate.outgoing_only {
        StartupServicePlan {
            start_server: false,
            request_mode: Some(Mode::Control.as_str()),
        }
    } else if gate.stop_service {
        StartupServicePlan {
            start_server: false,
            request_mode: None,
        }
    } else {
        StartupServicePlan {
            start_server: true,
            request_mode: Some(Mode::Host.as_str()),
        }
    }
}

pub(crate) fn apply_startup_service(plan: &StartupServicePlan, start_server: impl FnOnce()) {
    if plan.start_server {
        start_server();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn parses_host_and_control_without_a_stored_page() {
        assert_eq!(
            Launch::parse(&args(&["--host"]), false)
                .unwrap()
                .unwrap()
                .mode,
            Mode::Host
        );
        assert_eq!(
            Launch::parse(&args(&["--control"]), false)
                .unwrap()
                .unwrap()
                .mode,
            Mode::Control
        );
        assert!(Launch::parse(&[], false).unwrap().is_none());
        assert!(Launch::parse(&args(&["--server"]), false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejects_conflicting_and_extra_arguments() {
        for arguments in [
            args(&["--host", "--control"]),
            args(&["--control", "--host"]),
            args(&["--host", "payload"]),
            args(&["--control", "one", "two"]),
            args(&["--control", ""]),
        ] {
            assert!(Launch::parse(&arguments, false).is_err());
        }
        assert!(Launch::parse(&args(&["--host"]), true).is_err());
        assert!(Launch::parse(&args(&["--control"]), true).is_ok());
    }

    fn secret_pairing() -> (PairingCode, String) {
        let pairing = PairingCode {
            address: "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-".to_owned(),
            password: Some("秘密 &?= ;:\\,".to_owned()),
            name: Some("工作电脑".to_owned()),
            expires_at: None,
        };
        let encoded = pairing.encode().unwrap();
        (pairing, encoded)
    }

    #[test]
    fn rejects_inline_password_and_expired_codes_without_echoing_input() {
        let (_pairing, encoded) = secret_pairing();
        let error = Launch::parse(&args(&["--control", &encoded]), false)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("秘密"));
        assert!(!error.contains(&encoded));
        assert!(error.contains("SEALDESK_CONTROL_CODE"));
        assert!(error.contains("--control-file"));
        assert!(error.contains("--control -"));

        let passwordless = PairingCode {
            address: "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-".to_owned(),
            password: None,
            name: Some("工作电脑".to_owned()),
            expires_at: None,
        };
        let encoded = passwordless.encode().unwrap();
        let launch = Launch::parse(&args(&["--control", &encoded]), false)
            .unwrap()
            .unwrap();
        assert_eq!(launch.input.as_deref(), Some(encoded.as_str()));
        let expired = format!("{}E:1;;", encoded.trim_end_matches(';').to_owned() + ";");
        let error = Launch::parse(&args(&["--control", &expired]), false)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains(&encoded));
        assert!(error.contains("expired"));
    }

    #[test]
    fn reads_control_code_from_stdin_file_and_env_without_putting_it_on_the_command_line() {
        let (_pairing, encoded) = secret_pairing();
        let mut stdin = std::io::Cursor::new(format!("{encoded}\n"));
        let launch = Launch::parse_from(&args(&["--control", "-"]), false, None, &mut stdin)
            .unwrap()
            .unwrap();
        assert_eq!(launch.input.as_deref(), Some(encoded.as_str()));
        let command = flutter_control_arguments(launch.input.as_deref().unwrap());
        assert_eq!(command, vec!["--control".to_owned()]);
        assert!(command
            .iter()
            .all(|arg| !arg.contains("秘密") && arg != &encoded));

        let from_env = Launch::parse_from(
            &args(&["--control"]),
            false,
            Some(encoded.clone()),
            &mut std::io::Cursor::new([]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(from_env.input.as_deref(), Some(encoded.as_str()));
        assert!(
            !flutter_control_arguments(from_env.input.as_deref().unwrap())
                .iter()
                .any(|arg| arg.contains("秘密"))
        );

        let directory = std::env::temp_dir().join(format!(
            "sealdesk-control-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("code.txt");
        std::fs::write(&path, &encoded).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            let error = Launch::parse(&args(&["--control-file", path.to_str().unwrap()]), false)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("0600") || error.contains("权限"));
            assert!(!error.contains("秘密"));
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        let launch = Launch::parse(&args(&["--control-file", path.to_str().unwrap()]), false)
            .unwrap()
            .unwrap();
        assert_eq!(launch.input.as_deref(), Some(encoded.as_str()));
        assert!(flutter_control_arguments(launch.input.as_deref().unwrap())
            .iter()
            .all(|arg| !arg.contains("秘密")));

        #[cfg(unix)]
        {
            let link = directory.join("link.txt");
            std::os::unix::fs::symlink(&path, &link).unwrap();
            let error = Launch::parse(&args(&["--control-file", link.to_str().unwrap()]), false)
                .err()
                .unwrap()
                .to_string();
            assert!(error.contains("符号链接"));
            assert!(!error.contains("秘密"));
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn control_file_stat_rejects_non_owner_symlink_and_group_bits() {
        let owner = ControlFileStat {
            is_symlink: false,
            is_file: true,
            uid: 10,
            euid: 10,
            mode: 0o600,
        };
        assert!(control_file_stat_error(owner).is_none());
        assert!(control_file_stat_error(ControlFileStat {
            mode: 0o400,
            ..owner
        })
        .is_none());
        assert!(control_file_stat_error(ControlFileStat {
            mode: 0o644,
            ..owner
        })
        .is_some());
        assert!(control_file_stat_error(ControlFileStat { uid: 11, ..owner }).is_some());
        assert!(control_file_stat_error(ControlFileStat {
            is_symlink: true,
            is_file: false,
            ..owner
        })
        .is_some());
        assert!(control_file_stat_error(ControlFileStat {
            is_file: false,
            ..owner
        })
        .is_some());
    }

    #[test]
    fn malformed_password_field_is_not_a_password_code() {
        let address = "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-";
        let unterminated = format!("SEALDESK:V:1;A:{address};P:dummy-secret");
        let missing_terminator = format!("SEALDESK:V:1;A:{address};P:dummy-secret;");
        assert!(!pairing_declares_password(&unterminated));
        assert!(!pairing_declares_password(&missing_terminator));
        let error = Launch::parse(&args(&["--control", &missing_terminator]), false)
            .err()
            .unwrap()
            .to_string();
        assert!(!error.contains("dummy-secret"));
        let (_pairing, encoded) = secret_pairing();
        assert!(pairing_declares_password(&encoded));
    }

    #[test]
    fn windows_control_file_error_points_at_stdin_and_env() {
        assert!(CONTROL_FILE_WINDOWS_ERROR.contains("--control -"));
        assert!(CONTROL_FILE_WINDOWS_ERROR.contains("SEALDESK_CONTROL_CODE"));
        assert!(CONTROL_FILE_WINDOWS_ERROR.contains("Windows"));
    }

    #[test]
    fn staged_handoff_keeps_the_secret_out_of_ui_argv() {
        let (_pairing, encoded) = secret_pairing();
        for source in [
            ControlInputSource::Stdin,
            ControlInputSource::File,
            ControlInputSource::Env,
            ControlInputSource::Inline,
        ] {
            let handoff = control_ui_handoff(&encoded, source);
            assert_eq!(
                handoff.staged_env.as_deref(),
                Some(encoded.as_str()),
                "{source:?}"
            );
            assert!(handoff.replace_process_argv, "{source:?}");
            assert_eq!(handoff.argv, vec!["--control".to_owned()], "{source:?}");
            assert!(handoff
                .argv
                .iter()
                .all(|arg| arg != &encoded && !arg.contains("秘密")));
        }

        let passwordless = PairingCode {
            address: "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-".to_owned(),
            password: None,
            name: None,
            expires_at: None,
        };
        let encoded = passwordless.encode().unwrap();
        let from_stdin = control_ui_handoff(&encoded, ControlInputSource::Stdin);
        assert_eq!(from_stdin.staged_env.as_deref(), Some(encoded.as_str()));
        assert_eq!(from_stdin.argv, vec!["--control".to_owned()]);
        let inline = control_ui_handoff(&encoded, ControlInputSource::Inline);
        assert!(inline.staged_env.is_none());
        assert_eq!(inline.argv, vec!["--control".to_owned(), encoded.clone()]);

        let connect = non_flutter_connect(&secret_pairing().1)
            .err()
            .unwrap()
            .to_string();
        assert!(!connect.contains("秘密"));
        assert!(connect.contains("SEALDESK_CONTROL_CODE"));
        let plain = non_flutter_connect(&encoded).unwrap();
        assert_eq!(plain.address, passwordless.address);
        assert!(plain.name.is_none());
    }

    #[test]
    fn control_marker_argv_zeroes_the_value_that_followed() {
        fn c_buf(text: &str) -> Vec<u8> {
            let mut bytes = text.as_bytes().to_vec();
            bytes.push(0);
            bytes
        }
        let mut exe = c_buf("sealdesk");
        let mut flag = c_buf("--control-file");
        let mut value = c_buf("dummy-secret-value");
        let value_len = b"dummy-secret-value".len();
        let value_ptr = value.as_ptr();
        let mut argv = [
            exe.as_mut_ptr().cast(),
            flag.as_mut_ptr().cast(),
            value.as_mut_ptr().cast(),
            std::ptr::null_mut(),
        ];
        let mut argc: std::os::raw::c_int = 3;
        apply_control_marker_argv(&mut argc, argv.as_mut_ptr());
        assert_eq!(argc, 2);
        let marker = unsafe { std::ffi::CStr::from_ptr(argv[1]) };
        assert_eq!(marker.to_bytes(), b"--control");
        let wiped = unsafe { std::slice::from_raw_parts(value_ptr, value_len) };
        assert!(wiped.iter().all(|byte| *byte == 0));
        assert!(argv[2].is_null());
    }

    #[test]
    fn image_metadata_rejects_missing_directory_empty_and_oversized_files() {
        let directory = std::env::temp_dir().join(format!(
            "sealdesk-launch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let image = directory.join("配对 图.png");
        for path in [directory.as_path(), image.as_path()] {
            assert!(Launch::parse(&args(&["--control", path.to_str().unwrap()]), false).is_err());
        }
        let file = std::fs::File::create(&image).unwrap();
        assert!(Launch::parse(&args(&["--control", image.to_str().unwrap()]), false).is_err());
        file.set_len(MAX_IMAGE_BYTES + 1).unwrap();
        assert!(Launch::parse(&args(&["--control", image.to_str().unwrap()]), false).is_err());
        file.set_len(1).unwrap();
        let result = Launch::parse(&args(&["--control", image.to_str().unwrap()]), false);
        drop(file);
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(result.is_ok(), cfg!(feature = "flutter"));
    }

    fn start_server(calls: &mut usize) {
        *calls += 1;
    }

    fn times_start_server_called(plan: &StartupServicePlan) -> usize {
        let mut calls = 0;
        apply_startup_service(plan, || start_server(&mut calls));
        calls
    }

    #[test]
    fn desktop_start_calls_start_server_unless_explicitly_blocked() {
        use super::super::host::{host_request_action, HostRequest};

        let open = HostServiceGate {
            explicit_control: false,
            no_server: false,
            outgoing_only: false,
            stop_service: false,
        };
        let plan = plan_startup_service(open);
        assert_eq!(plan.request_mode, Some("host"));
        assert_eq!(times_start_server_called(&plan), 1);
        assert!(matches!(
            host_request_action(plan.request_mode.unwrap(), false, true).unwrap(),
            HostRequest::StartServer
        ));

        for (label, gate) in [
            (
                "--control",
                HostServiceGate {
                    explicit_control: true,
                    ..open
                },
            ),
            (
                "--no-server",
                HostServiceGate {
                    no_server: true,
                    ..open
                },
            ),
            (
                "outgoing-only",
                HostServiceGate {
                    outgoing_only: true,
                    ..open
                },
            ),
            (
                "stop-service",
                HostServiceGate {
                    stop_service: true,
                    ..open
                },
            ),
        ] {
            let plan = plan_startup_service(gate);
            assert_eq!(times_start_server_called(&plan), 0, "{label}");
            if gate.stop_service {
                assert_eq!(plan.request_mode, None, "{label}");
            } else {
                assert_eq!(plan.request_mode, Some("control"), "{label}");
                assert!(matches!(
                    host_request_action(plan.request_mode.unwrap(), gate.outgoing_only, true)
                        .unwrap(),
                    HostRequest::ControlWithoutServer
                ));
            }
        }
    }
}
