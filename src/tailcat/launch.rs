use super::pairing::PairingCode;
use hbb_common::{anyhow::anyhow, bail, ResultType};
use std::{
    fs::OpenOptions,
    io::{BufRead, Read},
};

pub(crate) const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
pub(crate) const CONTROL_CODE_ENV: &str = "SEALDESK_CONTROL_CODE";
const MAX_CONTROL_TEXT_BYTES: u64 = 16 * 1024;
const CONTROL_USAGE: &str = "用法：--host，或 --control [不含密码的配对码 | 二维码图片 | -]，或 --control-file <配对码文件>；含密码的配对码请从标准输入、限制权限的文件或环境变量 SEALDESK_CONTROL_CODE 读取；--host 不能与 --no-server 同用";
const INLINE_SECRET_ERROR: &str = "配对码含有密码，不能写在命令行参数里。请改用 --control - 从标准输入读取，或 --control-file <模式为 0600 且属于当前用户的文件>，或环境变量 SEALDESK_CONTROL_CODE";

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

pub(crate) struct Launch {
    pub(crate) mode: Mode,
    pub(crate) input: Option<String>,
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

/// UI handoff for a password-bearing code. Command lines stay free of that value.
pub(crate) fn stage_control_secret_for_ui(input: &str) {
    if pairing_declares_password(input) {
        std::env::set_var(CONTROL_CODE_ENV, input);
    } else {
        std::env::remove_var(CONTROL_CODE_ENV);
    }
}

/// Arguments for the Flutter UI. A password-bearing pairing code is never included.
pub(crate) fn flutter_control_arguments(input: &str) -> Vec<String> {
    if pairing_declares_password(input) {
        vec!["--control".to_owned()]
    } else {
        vec!["--control".to_owned(), input.to_owned()]
    }
}

fn pairing_prefix(input: &str) -> bool {
    input
        .trim()
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("SEALDESK:"))
}

fn pairing_declares_password(input: &str) -> bool {
    let input = input.trim();
    if !pairing_prefix(input) {
        return false;
    }
    let mut field = String::new();
    let mut escaped = false;
    for character in input[9..].chars() {
        if escaped {
            field.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == ';' {
            if field.split_once(':').is_some_and(|(key, _)| key == "P") {
                return true;
            }
            field.clear();
            continue;
        }
        field.push(character);
    }
    false
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
fn file_uid(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.uid()
}

#[cfg(not(unix))]
fn file_uid(_metadata: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn current_euid() -> u32 {
    unsafe { hbb_common::libc::geteuid() }
}

#[cfg(not(unix))]
fn current_euid() -> u32 {
    0
}

#[cfg(unix)]
fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    metadata.mode() & 0o777
}

#[cfg(not(unix))]
fn file_mode(_metadata: &std::fs::Metadata) -> u32 {
    0
}

fn metadata_stat(metadata: &std::fs::Metadata) -> ControlFileStat {
    ControlFileStat {
        is_symlink: metadata.file_type().is_symlink(),
        is_file: metadata.file_type().is_file(),
        uid: file_uid(metadata),
        euid: current_euid(),
        mode: file_mode(metadata),
    }
}

fn read_control_file(path: &str) -> ResultType<String> {
    let inspected = std::fs::symlink_metadata(path)
        .map_err(|_| anyhow!("无法读取配对码文件，请检查路径和权限"))?;
    if let Some(error) = control_file_stat_error(metadata_stat(&inspected)) {
        bail!(error);
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(hbb_common::libc::O_NOFOLLOW | hbb_common::libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| anyhow!("无法读取配对码文件，请检查路径和权限"))?
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|_| anyhow!("无法读取配对码文件，请检查路径和权限"))?;
    let metadata = file
        .metadata()
        .map_err(|_| anyhow!("无法读取配对码文件，请检查路径和权限"))?;
    if let Some(error) = control_file_stat_error(metadata_stat(&metadata)) {
        bail!(error);
    }
    if metadata.len() == 0 || metadata.len() > MAX_CONTROL_TEXT_BYTES {
        bail!("配对码文件必须是非空文本，且不能超过 16 KB");
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_CONTROL_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONTROL_TEXT_BYTES {
        bail!("配对码文件必须是非空文本，且不能超过 16 KB");
    }
    let text = String::from_utf8(bytes).map_err(|_| anyhow!("配对码文件不是有效的 UTF-8 文本"))?;
    accept_control_text(text)
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
            return Ok(Some(Self {
                mode: Mode::Control,
                input,
            }));
        };
        if input == "-" {
            return Ok(Some(Self {
                mode: Mode::Control,
                input: Some(read_control_line(stdin)?),
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
