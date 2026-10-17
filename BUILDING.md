# Building SealDesk

Run commands from the repository root unless a section says otherwise.
Builds require network access to fetch dependencies; offline LAN operation does
not imply that a fresh source build can run offline.

## Toolchains and native dependencies

- Rust and Cargo: use the target platform's configured toolchain in
  [.github/workflows/flutter-build.yml](.github/workflows/flutter-build.yml).
  The manifest's `rust-version` is not a guarantee that every locked dependency
  builds on that oldest compiler.
- Go: the version in [libs/tailcat-ffi/go.mod](libs/tailcat-ffi/go.mod), currently
  1.27.1. Set `GO=/path/to/go` if the required executable is not on `PATH`.
- Flutter: the default native workflow uses 3.24.5. Its Windows ARM64 variant
  uses a different SDK and patches; do not mix those bridge artifacts.
- C/C++ compiler, CMake, Ninja, pkg-config, Clang/libclang, and Python 3.
- Linux: GTK3, X11/XCB, audio/PulseAudio/ALSA, GStreamer development packages,
  plus the native codec libraries in [vcpkg.json](vcpkg.json). Set `VCPKG_ROOT`
  to a prepared vcpkg checkout. Follow the workflow's platform-specific packages,
  pinned vcpkg revision, and triplet rather than an old upstream README.
- Android: Android SDK, NDK r28c, Java 21, `cargo-ndk`, and the matching Rust
  Android target. Set `ANDROID_HOME` and `ANDROID_NDK_HOME`.

`libs/hbb_common` is vendored. There are no Git submodules to initialize in the
current checkout. The Go transport build details are in
[libs/tailcat-ffi/README.md](libs/tailcat-ffi/README.md).

## Generated bridge files

The fresh checkout intentionally excludes generated bridge artifacts, including:

- `src/bridge_generated.rs` and `src/bridge_generated.io.rs`
- `flutter/lib/generated_bridge.dart` and its Freezed companion
- the generated platform bridge headers

Use artifacts matching the source interfaces and target Flutter SDK.
[bridge.yml](.github/workflows/bridge.yml) records the pinned generation recipe
(`flutter_rust_bridge_codegen` 1.80.1). It is a reusable workflow called by the
application workflows; it is not a standalone manual-dispatch job. Do not use
an arbitrary current code generator or silently reuse an upstream interface.

`src/main.rs` is a retained native diagnostic entry, not the Flutter desktop
application. `cargo build --bin sealdesk` alone does not produce the GUI described
in the README.

## Linux x64 desktop / Debian package

With native dependencies and matching generated bridge files already prepared:

```bash
cargo build --locked --release --lib --features flutter,linux-pkg-config,inline
python3 build.py --flutter --skip-cargo
```

This is the software-codec Linux path; `linux-pkg-config` uses system libraries
where supported, while the remaining native dependencies still require the
configured vcpkg installation. Hardware codecs are a separate build choice.
`--skip-cargo` packages the just-built library; do not use it with stale native
output or after changing build features.

On Debian/Ubuntu the wrapper writes `sealdesk-<version>.deb` in the repository
root. The Flutter bundle is under `flutter/build/linux/x64/release/bundle/`.
The package does not enable a background daemon or autostart.

## Android arm64

The commands below use a Linux x64 build host.
With the same source-matched bridge files, native dependencies, SDK/NDK, Java,
and external signing configuration prepared:

```bash
rustup target add aarch64-linux-android
export BINDGEN_EXTRA_CLANG_ARGS="--target=aarch64-linux-android21 --sysroot=$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot"
TAILCAT_CC="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang" \
cargo ndk --platform 21 --target aarch64-linux-android build --locked --release --lib --features flutter
bash flutter/prepare_android_jni.sh arm64-v8a aarch64-linux-android
cd flutter
flutter build apk --release --target-platform android-arm64 --split-per-abi
```

For cgo cross-compilation, select the NDK compiler and sysroot as described in the
[transport build guide](libs/tailcat-ffi/README.md#交叉编译); the selected compiler
must target Android API 21. Native bindgen also needs the target NDK sysroot.
The JNI staging script includes `libsealdesk.so`, `libtailcat.so`, and the NDK C++
runtime. Building only one ABI and then requesting a multi-ABI package is not
supported by this recipe.

The APK is written to
`flutter/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk`.
From the repository root, check its native contents:

```bash
python3 tests/check_sealdesk_android_package.py flutter/build/app/outputs/flutter-apk/app-arm64-v8a-release.apk --abi arm64-v8a
```

## Android release signing

Keep the keystore and properties file outside the checkout. The default is
`~/.sealdesk/key.properties`; `SEALDESK_KEY_PROPERTIES` can override it.
Create a protected directory and generate your own key, retaining a safe backup:

```bash
mkdir -p ~/.sealdesk
chmod 700 ~/.sealdesk
keytool -genkeypair -v \
  -keystore ~/.sealdesk/release.jks \
  -alias sealdesk \
  -keyalg RSA -keysize 2048 -validity 10000
chmod 600 ~/.sealdesk/release.jks
```

Write `key.properties` with your own values:

```properties
storeFile=/absolute/path/outside/checkout/release.jks
storePassword=<keystore-password>
keyAlias=sealdesk
keyPassword=<key-password>
```

Protect it with `chmod 600 ~/.sealdesk/key.properties`. Do not commit signing
keys, passwords, local SDK configuration, or build outputs. Release Gradle tasks
reject missing signing configuration; debug tasks do not require release keys.
Set `SEALDESK_ANDROID_ALLOW_UNSIGNED=1` (environment variable or Gradle property)
to build an unsigned release APK when signing is absent. CI sets that opt-in
only when `ANDROID_SIGNING_KEY` is empty. Flutter still copies the unsigned
APK to `app-<abi>-release.apk` or `app-release.apk`.

## Validation scope

Linux x64 and Android arm64 have been built during development. The source
contains additional platform recipes, not a promise that all of them currently
produce validated SealDesk packages. In particular, a transport-library test
does not establish full Windows, macOS, or iOS application support.

A binary release must identify its source revision, preserve the project and
dependency notices, and provide the corresponding build sources. See
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
