# SealDesk 的 Go 隧道构建

完整应用的构建步骤见 [BUILDING.md](../../BUILDING.md)；这里说明传输库，
不把原生诊断二进制当作 Flutter 桌面应用。

Rust 的根目录 `build.rs` 在桌面与 iOS 上构建静态库，在 Android 上构建并动态链接
`libtailcat.so`。Android 的动态库同时复制到 Cargo 对应 target/profile 目录，
供应用打包流程与 Rust 库一起收集。
Go 版本以 `go.mod` 为准；目前要求 Go 1.27.1。原生 Flutter 应用还需要项目 CI
生成的 Rust/Dart 桥接产物，以及 RustDesk 原有的平台依赖。

## 本机构建

在仓库根目录运行：

```bash
GO=/path/to/go cargo build --locked --lib
```

`GO` 省略时使用 PATH 中的 Go。cgo 始终开启，需要目标平台的 C 编译器；
Windows 上不能将 MSVC 的 `cl.exe` 当作 Go 的 GCC/Clang 编译器使用。
Windows 完整程序的编译器与链接兼容性仍需要在对应平台实际验收。

## 构建目标

构建入口从 Cargo 的 `CARGO_CFG_TARGET_OS` 和 `CARGO_CFG_TARGET_ARCH` 确定
`GOOS`、`GOARCH`，覆盖终端继承的同名变量，避免 Go 库与 Rust 程序架构不同。

| Rust 平台 / 架构 | Go 设置 |
|---|---|
| Linux | `GOOS=linux` |
| macOS | `GOOS=darwin` |
| Windows | `GOOS=windows` |
| Android | `GOOS=android` |
| iOS（含模拟器） | `GOOS=ios` |
| x86_64 | `GOARCH=amd64` |
| x86 | `GOARCH=386` |
| aarch64 | `GOARCH=arm64` |
| armv7 目标 | `GOARCH=arm GOARM=7` |

以上是构建参数映射，不能替代各平台编译、链接和运行验收。未覆盖的架构提前报错。

## 交叉编译

当 Cargo 的 `HOST` 和 `TARGET` 不同时，必须显式指定目标 C 编译器。
构建入口按以下优先级选择首个非空值，并原样传给 Go 的 `CC`：

1. `TAILCAT_CC`
2. `CC_<完整 Rust target>`
3. `CC_<将 target 中连字符替换为下划线>`
4. `TARGET_CC`
5. `CC`

编译器必须包含正确的目标、SDK、sysroot 和最低系统版本参数；如需额外的
预处理、编译或链接选项，使用 Go 的 `CGO_CPPFLAGS`、`CGO_CFLAGS`、
`CGO_CXXFLAGS`、`CGO_LDFLAGS`。这些变量及编译器选择变化会触发静态库重建。
不自动把 Rust 链接器当作 C 编译器。

在已配置 Android NDK、Rust 目标、Flutter 桥接产物及项目其他依赖的 Linux 主机上：

```bash
TAILCAT_CC="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android21-clang" \
GO=/path/to/go \
cargo build --locked --release --lib --target aarch64-linux-android --features flutter
```

该例只说明 cgo 的编译器选择，不负责安装 NDK、生成 Flutter 桥接文件或配置
vcpkg。iOS 真机和模拟器须分别选择对应 SDK；Windows、macOS 和移动端成品
仍需在各自工具链与运行环境中验证。

## 检查

在仓库根目录运行：

```bash
GO=/path/to/go cargo test --lib tailcat::
/path/to/go -C libs/tailcat-ffi test -race ./...
```

目标映射、错误的继承环境、交叉编译缺少编译器和 ARMv7 设置的回归测试位于
`src/tailcat/build_target.rs`，已挂到正常 Rust 测试入口。

## 独立桌面隧道 CI

`.github/workflows/sealdesk-transport.yml` 定义 Linux x86-64、macOS Intel、
macOS Apple Silicon 和 Windows x86-64 的本机静态库检查。它从本目录的
`go.mod` 选择 Go 版本；Windows 使用 MSYS2 UCRT64 GCC。每个任务先运行
Go 竞态测试，再构建 `libtailcat.a` 和 `libtailcat.h`，编译并运行调用这些
导出符号的 C 程序，成功后上传静态库、头文件与工具链信息，保留 14 天。

本机可运行同一个入口：

```bash
GO=/path/to/go bash tests/build_tailcat_archive.sh /tmp/tailcat-archive
```

C 链接探针默认使用 `cc`；可用 `TAILCAT_SMOKE_CC` 指定编译器可执行文件路径。
该变量只接受可执行文件路径，额外 cgo 编译选项仍按前文设置。脚本拒绝在
本机直接执行其他架构的产物，不需要网络服务、配对码或现有被控服务。

这些产物只验证 Go/C ABI。Windows GNU 编译器检查不能替代正式 Rust MSVC
应用链接验收；Linux CI 的 Ubuntu 24.04 基线也不承诺旧系统兼容性。完整应用的
桥接生成、Flutter 打包、各平台签名与运行验收仍由应用构建流程负责。

## Android 动态库与链接检查

当前 Go 1.27.1 对 `GOOS=android GOARCH=arm64 -buildmode=c-archive` 明确返回
`-buildmode=c-archive not supported on android/arm64`。因此 Android 构建入口使用
`c-shared`，Cargo 使用 `dylib=tailcat`。桌面和 iOS 的静态库格式保持原有选择。

正式 Android CI 将 `libtailcat.so` 复制到对应 ABI 的 `jniLibs` 目录；正式 CI 的原生库
artifact 同时包含 Rust 与 Go 两个库，供通用 APK 任务下载。使用这些 artifact 的其他构建流程
也必须保留 Go 库，不能只取 Rust 库。主 Android CI 显式安装 Go；本地环境也需要可用的 Go 工具链。

独立隧道 CI 还定义 ARM64、ARMv7、x86-64、x86 四个 Android 构建任务，使用 NDK r28c
与 API 21 编译动态库，并用实际 Rust Android target 编译调用导出函数的动态库，检查
其 ELF 依赖中包含 `libtailcat.so`。可在 Linux 上运行同一检查：

```bash
ANDROID_NDK_HOME=/path/to/android-ndk-r28c \
GO=/path/to/go \
bash tests/build_tailcat_android.sh aarch64-linux-android /tmp/tailcat-android
```

检查前需通过 rustup 安装对应 Rust Android target。该检查只构建与链接库，不运行
Android 二进制，也不生成完整 APK。JNI 库实际入包、设备动态加载、16 KB 页面设备、
扫码和远程画面均仍需完整应用与真机验收。

## APK 原生库入包检查

Android 正式分架构包和通用包在打包后、签名发布前执行
`tests/check_sealdesk_android_package.py`。检查 APK 内实际出现的 ABI 是否与预期完全
一致，每个 ABI 是否恰好包含一份 `libsealdesk.so` 和 `libtailcat.so`，两个库的
ELF 架构与动态库类型是否正确，以及 Rust 库是否声明对 Go 库的动态依赖。

检查使用 Python 3.8+ 标准库及 binutils 的 `readelf`；也可用 `--readelf` 指定 NDK
中的 `llvm-readelf`。例如检查 ARM64 单架构 APK：

```bash
python3 tests/check_sealdesk_android_package.py path/to/app-arm64-v8a-release.apk \
  --abi arm64-v8a
```

通用包重复传入 `--abi` 指定每种预期架构。独立 Android 构建脚本还会用真实交叉
编译的 Go 库和 Rust 链接探针创建临时 ZIP 夹具，执行三项回归测试，覆盖正常入包、
漏库、ABI 集合不符、ELF 架构错误和缺失动态依赖。这些夹具没有 Android 应用内容，
不代表完整 APK 已成功构建或通过设备加载测试。

## 撤销旧地址

桌面版本可运行 `sealdesk --tc-reset-address`（本地开发文件名为
`flutter/build/linux/x64/release/bundle/sealdesk`）。该命令会断开当前被控连接，并使所有包含旧 tc 地址的
配对码失效；原来运行的被控服务随后生成新密钥和地址。原来停止的服务或控制模式
保持停止，控制端身份密钥和白名单保持不变。请重新获取配对码后分享。

重置请求由本机服务删除其自己的服务端密钥文件，并返回确认；服务未停止时拒绝
删除。命令不会启动一个尚未运行的本机服务，失败会以非零状态退出。桌面被控页
也提供“重置地址”按钮：确认后隐藏旧码，等待新地址才显示新码；失败时提示原因并
允许重试。禁止修改设置时隐藏按钮。完整原生界面联调仍须匹配版本的 CI 桥接产物。

## 桌面剪贴板图片导入

控制页的“粘贴二维码图片”读取本机剪贴板中的 PNG 或位图，识别后使用现有配对码
连接流程。错误提示区会显示没有图片、尺寸超限、格式错误、无法识别或配对码过期
的原因。图片限制为 20 MB 编码数据、单边 8192 和总计 16 M 像素，内容仅用于本次导入。

Linux 原生读取可在隔离 X11 桌面中复测；需测试环境提供 Xvfb 和 xauth：

```bash
env -u WAYLAND_DISPLAY xvfb-run -a -s '-screen 0 800x600x24 -nolisten tcp' \
  env SEALDESK_CLIPBOARD_PROBE=1 \
  cargo test --lib tailcat::clipboard::tests::live_clipboard_image \
  -- --ignored --exact --nocapture --test-threads=1
```

原生读取和 Flutter 页面分别有测试。完整 GUI 联调、Wayland、Windows 和 macOS
系统剪贴板仍需实际应用验收；网页和移动端继续使用图片文件、相册或扫码入口。
