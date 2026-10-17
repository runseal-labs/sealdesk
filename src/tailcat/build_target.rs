use std::{ffi::OsStr, process::Command};

pub(crate) fn library_format(target_os: &str) -> (&'static str, &'static str, &'static str) {
    if target_os == "android" {
        ("c-shared", "libtailcat.so", "dylib")
    } else if target_os == "windows" {
        // A c-archive inside the MSVC-linked DLL never runs the Go runtime's
        // gcc constructors, so the first call into Go blocks forever.
        ("c-shared", "tailcat.dll", "dylib")
    } else {
        ("c-archive", "libtailcat.a", "static")
    }
}

pub(crate) fn compiler_keys(target: &str) -> Vec<String> {
    vec![
        "TAILCAT_CC".to_owned(),
        format!("CC_{target}"),
        format!("CC_{}", target.replace('-', "_")),
        "TARGET_CC".to_owned(),
        "CC".to_owned(),
    ]
}

pub(crate) fn configure(
    command: &mut Command,
    host: &str,
    target: &str,
    target_os: &str,
    target_arch: &str,
    compiler: Option<&OsStr>,
) -> Result<(), String> {
    let go_os = match target_os {
        "linux" | "windows" | "android" | "ios" => target_os,
        "macos" => "darwin",
        _ => return Err(format!("Unsupported tailcat target OS: {target_os}")),
    };
    let go_arch = match target_arch {
        "x86_64" => "amd64",
        "x86" => "386",
        "aarch64" => "arm64",
        "arm" if target.starts_with("armv7") => "arm",
        _ => return Err(format!("Unsupported tailcat target architecture: {target}")),
    };
    let compiler = compiler.filter(|value| !value.is_empty());
    if host != target && compiler.is_none() {
        return Err(format!(
            "Cross-compiling tailcat for {target} requires a target C compiler; set TAILCAT_CC or CC_{} (including the SDK/sysroot arguments)",
            target.replace('-', "_")
        ));
    }
    command
        .env("GOOS", go_os)
        .env("GOARCH", go_arch)
        .env("CGO_ENABLED", "1");
    if go_arch == "arm" {
        command.env("GOARM", "7");
    } else {
        command.env_remove("GOARM");
    }
    if let Some(compiler) = compiler {
        command.env("CC", compiler);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_uses_shared_library_and_desktop_keeps_static_archive() {
        assert_eq!(
            library_format("android"),
            ("c-shared", "libtailcat.so", "dylib")
        );
        assert_eq!(
            library_format("windows"),
            ("c-shared", "tailcat.dll", "dylib")
        );
        for target_os in ["linux", "macos", "ios"] {
            assert_eq!(
                library_format(target_os),
                ("c-archive", "libtailcat.a", "static")
            );
        }
    }

    #[test]
    fn cargo_targets_override_inherited_go_target_settings() {
        for (target, target_os, target_arch, go_os, go_arch) in [
            (
                "x86_64-unknown-linux-gnu",
                "linux",
                "x86_64",
                "linux",
                "amd64",
            ),
            (
                "aarch64-apple-darwin",
                "macos",
                "aarch64",
                "darwin",
                "arm64",
            ),
            (
                "x86_64-pc-windows-msvc",
                "windows",
                "x86_64",
                "windows",
                "amd64",
            ),
            (
                "aarch64-linux-android",
                "android",
                "aarch64",
                "android",
                "arm64",
            ),
            ("aarch64-apple-ios-sim", "ios", "aarch64", "ios", "arm64"),
            ("i686-linux-android", "android", "x86", "android", "386"),
        ] {
            let mut command = Command::new("go");
            command
                .env("GOOS", "wrong")
                .env("GOARCH", "wrong")
                .env("GOARM", "5");
            configure(&mut command, target, target, target_os, target_arch, None).unwrap();
            let settings: std::collections::HashMap<_, _> = command.get_envs().collect();
            assert_eq!(settings[OsStr::new("GOOS")], Some(OsStr::new(go_os)));
            assert_eq!(settings[OsStr::new("GOARCH")], Some(OsStr::new(go_arch)));
            assert_eq!(settings[OsStr::new("CGO_ENABLED")], Some(OsStr::new("1")));
            assert_eq!(settings[OsStr::new("GOARM")], None);
        }
    }

    #[test]
    fn cross_compilation_requires_and_preserves_target_compiler() {
        let target = "armv7-linux-androideabi";
        let mut command = Command::new("go");
        assert!(configure(&mut command, "host", target, "android", "arm", None).is_err());
        assert!(configure(
            &mut command,
            "host",
            target,
            "android",
            "arm",
            Some(OsStr::new(""))
        )
        .is_err());
        let compiler =
            OsStr::new("clang --target=armv7a-linux-androideabi21 --sysroot='/sdk path'");
        configure(
            &mut command,
            "host",
            target,
            "android",
            "arm",
            Some(compiler),
        )
        .unwrap();
        let settings: std::collections::HashMap<_, _> = command.get_envs().collect();
        assert_eq!(settings[OsStr::new("GOARM")], Some(OsStr::new("7")));
        assert_eq!(settings[OsStr::new("CC")], Some(compiler));
        assert_eq!(compiler_keys(target).first().unwrap(), "TAILCAT_CC");
        assert!(compiler_keys(target).contains(&"CC_armv7_linux_androideabi".to_owned()));
    }

    #[test]
    fn unsupported_targets_fail_before_go_runs() {
        let mut command = Command::new("go");
        assert!(configure(&mut command, "wasm", "wasm", "unknown", "wasm32", None).is_err());
        assert!(configure(&mut command, "linux", "linux", "linux", "riscv32", None).is_err());
        assert!(configure(&mut command, "arm", "arm", "linux", "arm", None).is_err());
        assert_eq!(command.get_envs().count(), 0);
    }
}
