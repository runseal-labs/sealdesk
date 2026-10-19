use super::pairing::PairingCode;
use hbb_common::{bail, ResultType};

pub(crate) const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

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

impl Launch {
    pub(crate) fn parse(args: &[String], no_server: bool) -> ResultType<Option<Self>> {
        let Some(command) = args.first().map(String::as_str) else {
            return Ok(None);
        };
        let mode = match command {
            "--host" => Mode::Host,
            "--control" => Mode::Control,
            _ => return Ok(None),
        };
        if args.len() > 2 || (mode == Mode::Host && (args.len() != 1 || no_server)) {
            bail!("用法：--host 或 --control [配对码文本 | 二维码图片路径]；--host 不能与 --no-server 同用");
        }
        let input = args.get(1).cloned();
        if let Some(input) = &input {
            if input
                .trim()
                .get(..9)
                .map(|prefix| prefix.eq_ignore_ascii_case("SEALDESK:"))
                .unwrap_or(false)
            {
                PairingCode::parse(input)?;
            } else {
                if input.starts_with("--") || input.is_empty() {
                    bail!("--control 需要配对码文本或二维码图片路径");
                }
                let metadata = std::fs::metadata(input).map_err(|_| {
                    hbb_common::anyhow::anyhow!("无法读取二维码图片，请检查文件路径和权限")
                })?;
                if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_IMAGE_BYTES {
                    bail!("二维码图片必须是非空普通文件，且不能超过 20 MB");
                }
                #[cfg(not(feature = "flutter"))]
                bail!("二维码图片命令行导入需要 Flutter 版本；当前版本请使用配对码文本");
            }
        }
        Ok(Some(Self { mode, input }))
    }
}

/// Last UI page. Not whether the controlled service starts.
pub(crate) fn startup_mode(launch: Option<&Launch>, remembered: &str) -> Mode {
    if let Some(launch) = launch {
        return launch.mode;
    }
    if remembered == Mode::Control.as_str() {
        Mode::Control
    } else {
        Mode::Host
    }
}

pub(crate) fn controller_only(mode: &str, no_server: bool) -> bool {
    no_server || mode != Mode::Host.as_str()
}

/// Explicit blocks only. A remembered `control` page is not one of them.
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
    pub rewrite_remembered_mode: bool,
}

/// Desktop share card is always on screen, so a leftover `sealdesk-mode=control`
/// must not suppress the service and must not be rewritten.
pub(crate) fn plan_startup_service(
    remembered_mode: &str,
    gate: HostServiceGate,
) -> StartupServicePlan {
    let _ui_page = remembered_mode;
    if gate.explicit_control || gate.no_server || gate.outgoing_only {
        StartupServicePlan {
            start_server: false,
            request_mode: Some(Mode::Control.as_str()),
            rewrite_remembered_mode: false,
        }
    } else if gate.stop_service {
        StartupServicePlan {
            start_server: false,
            request_mode: None,
            rewrite_remembered_mode: false,
        }
    } else {
        StartupServicePlan {
            start_server: true,
            request_mode: Some(Mode::Host.as_str()),
            rewrite_remembered_mode: false,
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
    fn modes_and_remembered_controller_policy() {
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
        assert_eq!(startup_mode(None, ""), Mode::Host);
        assert_eq!(startup_mode(None, "host"), Mode::Host);
        assert_eq!(startup_mode(None, "control"), Mode::Control);
        assert_eq!(startup_mode(None, "nope"), Mode::Host);
        for (arguments, expected) in [
            (args(&["--host"]), Mode::Host),
            (args(&["--control"]), Mode::Control),
        ] {
            let launch = Launch::parse(&arguments, false).unwrap();
            assert_eq!(startup_mode(launch.as_ref(), "control"), expected);
            assert_eq!(startup_mode(launch.as_ref(), ""), expected);
        }
        assert!(Launch::parse(&args(&["--server"]), false)
            .unwrap()
            .is_none());
        assert!(controller_only("control", false));
        assert!(controller_only("host", true));
        assert!(!controller_only("host", false));
        assert!(controller_only("", false));
        assert!(controller_only("unknown", false));
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

    #[test]
    fn preserves_secrets_and_rejects_expired_codes_without_echoing_input() {
        let pairing = PairingCode {
            address: "tcABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdef_-".to_owned(),
            password: Some("秘密 &?= ;:\\,".to_owned()),
            name: Some("工作电脑".to_owned()),
            expires_at: None,
        };
        let encoded = pairing.encode().unwrap();
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
    fn remembered_control_calls_start_server_and_explicit_blocks_do_not() {
        use super::super::host::{host_request_action, HostRequest};

        let open = HostServiceGate {
            explicit_control: false,
            no_server: false,
            outgoing_only: false,
            stop_service: false,
        };
        for remembered in ["control", "host", ""] {
            let plan = plan_startup_service(remembered, open);
            assert!(!plan.rewrite_remembered_mode, "{remembered}");
            assert_eq!(plan.request_mode, Some("host"), "{remembered}");
            assert_eq!(times_start_server_called(&plan), 1, "{remembered}");
            assert!(matches!(
                host_request_action(plan.request_mode.unwrap(), false, true).unwrap(),
                HostRequest::StartServer
            ));
        }

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
            let plan = plan_startup_service("control", gate);
            assert!(!plan.rewrite_remembered_mode, "{label}");
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
