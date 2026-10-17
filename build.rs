#[path = "src/tailcat/build_target.rs"]
mod tailcat_build_target;

#[cfg(windows)]
fn build_windows() {
    let file = "src/platform/windows.cc";
    let file2 = "src/platform/windows_delete_test_cert.cc";
    cc::Build::new().file(file).file(file2).compile("windows");
    println!("cargo:rustc-link-lib=WtsApi32");
    println!("cargo:rerun-if-changed={}", file);
    println!("cargo:rerun-if-changed={}", file2);
}

#[cfg(target_os = "macos")]
fn build_mac() {
    let file = "src/platform/macos.mm";
    let mut b = cc::Build::new();
    if let Ok(os_version::OsVersion::MacOS(v)) = os_version::detect() {
        let v = v.version;
        if v.contains("10.14") {
            b.flag("-DNO_InputMonitoringAuthStatus=1");
        }
    }
    b.flag("-std=c++17").file(file).compile("macos");
    println!("cargo:rerun-if-changed={}", file);
}

#[cfg(all(windows, feature = "inline"))]
fn build_manifest() {
    use std::io::Write;
    if std::env::var("PROFILE").unwrap() == "release" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("res/icon.ico")
            .set_language(winapi::um::winnt::MAKELANGID(
                winapi::um::winnt::LANG_ENGLISH,
                winapi::um::winnt::SUBLANG_ENGLISH_US,
            ))
            .set_manifest_file("res/manifest.xml");
        match res.compile() {
            Err(e) => {
                write!(std::io::stderr(), "{}", e).unwrap();
                std::process::exit(1);
            }
            Ok(_) => {}
        }
    }
}

// bionic only exports getifaddrs()/freeifaddrs() from API 24, while the jniLibs
// are built against the API 21 sysroot (flutter/ndk_*.sh). webrtc-util calls
// them, so without this the android link fails on undefined symbols.
fn build_android_ifaddrs() {
    let file = "src/platform/android_ifaddrs.c";
    cc::Build::new().file(file).compile("android_ifaddrs");
    println!("cargo:rerun-if-changed={}", file);
}

fn install_android_deps() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os != "android" {
        return;
    }
    let mut target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if target_arch == "x86_64" {
        target_arch = "x64".to_owned();
    } else if target_arch == "x86" {
        target_arch = "x86".to_owned();
    } else if target_arch == "aarch64" {
        target_arch = "arm64".to_owned();
    } else {
        target_arch = "arm".to_owned();
    }
    let target = format!("{}-android", target_arch);
    let vcpkg_root = std::env::var("VCPKG_ROOT").unwrap();
    let mut path: std::path::PathBuf = vcpkg_root.into();
    if let Ok(vcpkg_root) = std::env::var("VCPKG_INSTALLED_ROOT") {
        path = vcpkg_root.into();
    } else {
        path.push("installed");
    }
    path.push(target);
    println!(
        "cargo:rustc-link-search={}",
        path.join("lib").to_str().unwrap()
    );
    println!("cargo:rustc-link-lib=ndk_compat");
    println!("cargo:rustc-link-lib=c++");
    println!("cargo:rustc-link-lib=OpenSLES");
}

// Builds libs/tailcat-ffi (Go) as a native library and links it in. The tunnel
// replaces the rendezvous and relay servers. Needs `go` on PATH (or $GO).
fn build_tailcat() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::path::Path::new("libs/tailcat-ffi");
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR")?);
    let target = std::env::var("TARGET")?;
    let host = std::env::var("HOST")?;
    let target_os = std::env::var("CARGO_CFG_TARGET_OS")?;
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH")?;
    let compiler_keys = tailcat_build_target::compiler_keys(&target);
    for key in compiler_keys.iter().map(String::as_str).chain([
        "GO",
        "GOFLAGS",
        "GOEXPERIMENT",
        "GOROOT",
        "GOTOOLCHAIN",
        "CGO_CFLAGS",
        "CGO_CPPFLAGS",
        "CGO_CXXFLAGS",
        "CGO_LDFLAGS",
        "SDKROOT",
    ]) {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let compiler = compiler_keys
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty());
    let go = std::env::var("GO").unwrap_or_else(|_| "go".to_owned());
    let mut command = std::process::Command::new(go);
    let (build_mode, library_name, link_kind) = tailcat_build_target::library_format(&target_os);
    tailcat_build_target::configure(
        &mut command,
        &host,
        &target,
        &target_os,
        &target_arch,
        compiler.as_deref(),
    )?;
    command
        .current_dir(dir)
        .arg("build")
        .arg(format!("-buildmode={build_mode}"));
    if target_os == "windows" {
        // GNU ld's import library is what MSVC link.exe resolves `dylib=tailcat` against.
        command.arg(format!(
            "-ldflags=-extldflags=-Wl,--out-implib,{}",
            out.join("tailcat.lib").display()
        ));
    }
    let status = command
        .args(["-trimpath", "-o"])
        .arg(out.join(library_name))
        .arg(".")
        .status()
        .map_err(|error| format!("Failed to run Go for {target}; install Go or set GO: {error}"))?;
    if !status.success() {
        return Err(format!("Building libs/tailcat-ffi for {target} failed ({status}); check the target cgo compiler and SDK").into());
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib={link_kind}=tailcat");
    if target_os == "android" || target_os == "windows" {
        let profile_dir = out
            .ancestors()
            .nth(3)
            .ok_or("Unexpected Cargo OUT_DIR layout")?;
        std::fs::copy(out.join(library_name), profile_dir.join(library_name))?;
    }
    if target_os == "macos" {
        println!("cargo:rustc-link-lib=framework=CoreFoundation");
        println!("cargo:rustc-link-lib=framework=Security");
        println!("cargo:rustc-link-lib=resolv");
    }
    for f in [
        "main.go",
        "transport.go",
        "client_cache.go",
        "go.mod",
        "go.sum",
    ] {
        println!("cargo:rerun-if-changed=libs/tailcat-ffi/{f}");
    }
    println!("cargo:rerun-if-changed=src/tailcat/build_target.rs");
    Ok(())
}

fn main() {
    hbb_common::gen_version();
    if let Err(error) = build_tailcat() {
        eprintln!("tailcat build failed: {error}");
        std::process::exit(1);
    }
    install_android_deps();
    #[cfg(all(windows, feature = "inline"))]
    build_manifest();
    #[cfg(windows)]
    build_windows();
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    if target_os == "macos" {
        #[cfg(target_os = "macos")]
        build_mac();
        println!("cargo:rustc-link-lib=framework=ApplicationServices");
    }
    if target_os == "android" {
        build_android_ifaddrs();
    }
    println!("cargo:rerun-if-changed=build.rs");
}
