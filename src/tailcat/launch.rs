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

pub(crate) fn startup_mode(launch: Option<&Launch>) -> Mode {
    launch.map(|launch| launch.mode).unwrap_or(Mode::Control)
}

pub(crate) fn controller_only(mode: &str, no_server: bool) -> bool {
    no_server || mode != Mode::Host.as_str()
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
        assert_eq!(startup_mode(None), Mode::Control);
        for (arguments, expected) in [
            (args(&["--host"]), Mode::Host),
            (args(&["--control"]), Mode::Control),
        ] {
            let launch = Launch::parse(&arguments, false).unwrap();
            assert_eq!(startup_mode(launch.as_ref()), expected);
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
}
