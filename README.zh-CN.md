# SealDesk

[English](README.md) | 简体中文

SealDesk 是一个远程桌面应用，可以在两台设备之间直接查看和控制屏幕，不用注册账号，也不依赖 RustDesk 的 ID 服务器或中继服务器。

> **当前为 alpha 预览版。** 功能和配置仍可能变化，目前优先适配 Linux x64 和 Android arm64，其他平台尚未完成验证。请不要用于关键生产环境。

## 它能做什么

- **零账号**：不需要登录、注册或自建 ID 服务器。
- **扫码连接**：被控端显示一个配对二维码（或 `SEALDESK:` 开头的配对码），控制端扫码或粘贴后即可连接。
- **直连优先**：底层使用 tailcat 传输库建立端到端加密连接，能直连时直连，网络受限时经 DERP 中继转发。
- **兼容原版 RustDesk**：局域网列表可连接开启了“允许直接 IP 访问”的原版设备，沿用其密码或点击授权。原版 IP 直连不加密，连接前必须确认；SealDesk 自有连接继续加密。
- **Android 局域网控制**：在原有“发现”标签页选择共享设备，复用加密直连、首次点击授权和已授权控制端自动接受逻辑。
- **局域网连接**：原生桌面端开启共享后，控制端可在“局域网设备”中选择并请求连接；首次由被控端点击接受。完全断公网、首次打开也可使用，传输仍然加密，不需要配对密码或额外启用 2FA。
- **记住控制端**：接受时勾选“信任此设备”，后续按已授予的权限自动接受；被控端可撤销授权，断开该控制端并要求重新确认。授权绑定密钥，不依赖设备名或 IP。
- **常用远程功能**：屏幕查看与控制、键鼠输入、音频、剪贴板和文件传输（沿用 RustDesk 的会话实现）。

SealDesk 与原版 RustDesk 客户端**不互通**。

## 使用方式

1. 在要被控制的设备上打开 SealDesk，进入被控（Host）模式，界面会显示配对二维码和配对码。
2. 在控制设备上打开 SealDesk，进入控制模式，扫描二维码或粘贴配对码。
3. 连接建立后即可远程操作。

**安全提示**：配对码就是访问凭证，请像密码一样对待，只发给你信任的人。如果担心配对码或 `tc...` 地址已泄露，请在被控端重置地址，旧的配对码会立即失效：

```bash
sealdesk --tc-reset-address
```

原版 RustDesk 也可以发现并控制 SealDesk：兼容入口为标准 TCP 21118，无需另填端口，沿用原版密码或点击授权，界面明确提示非加密。仅在“共享本机”开启时监听，关闭共享会断开兼容会话；两种协议共用 TCP 21118；SealDesk 先声明加密协议，握手失败直接断开，不回退明文。

局域网发现使用 UDP 21119，桌面连接共用 TCP 21118；仅在共享本机开启时监听，退出应用后停止。路由器若隔离无线客户端或禁止广播，设备可能无法自动发现。

## 安装

目前尚未发布正式安装包，alpha 阶段请从源码构建（见下文）。如果拿到了预构建的测试包：

- **Linux x64**：用系统包管理器安装，例如 `sudo apt install ./sealdesk-*.deb`。
- **Android arm64**：安装 `arm64-v8a` 的 APK（需允许“安装未知来源应用”），被控端按应用内提示授予所需权限。

## 从源码构建

### 准备

- Rust（1.75 或更新）
- Go，版本见 `libs/tailcat-ffi/go.mod`（构建时会通过 cgo 编译 tailcat 并链接进 SealDesk）
- C 编译器及目标平台的原生依赖
- 构建 Flutter 界面或 Android 包时还需要对应版本的 Flutter SDK 和 Android NDK

如果所需的 Go 不在 `PATH` 中，可以通过 `GO=/path/to/go` 指定。

### Linux 命令行版本（开发调试）

```bash
git submodule update --init --recursive
cargo build --bin sealdesk

./target/debug/sealdesk --host                              # 作为被控端
./target/debug/sealdesk --control 'SEALDESK:V:1;A:...;;'    # 用配对码连接
```

### Linux 桌面版（Flutter 界面）

```bash
python3 build.py --flutter --hwcodec
```

完整的 Flutter 构建还需要项目工具链生成的桥接代码。

### Android arm64

```bash
cd flutter
bash ndk_arm64.sh        # 编译原生库并放入 jniLibs
bash build_android.sh    # 打包 APK
```

发布版签名使用仓库外的密钥库和配置文件（默认 `~/.sealdesk/key.properties`，可用 `SEALDESK_KEY_PROPERTIES` 指定）。**不要把密钥库或密码提交进仓库或贴到日志里。** 调试构建不需要签名配置。

## 相关文档

- [FORK.md](FORK.md)：分叉边界与维护规则
- [docs/sealdesk-prd.md](docs/sealdesk-prd.md)：配对流程与产品行为
- [libs/tailcat-ffi/README.md](libs/tailcat-ffi/README.md)：tailcat 传输库

## Linux 桌面运行方式

Linux 安装包只提供普通桌面应用，不安装、启用或启动 systemd 守护服务。
需要时从应用菜单或终端运行 `sealdesk`。升级旧包时会停用并移除旧版
SealDesk 服务单元，不会改用其他后台自启动方式。

普通启动始终处于控制模式，即使上次退出前打开了共享，也需要再次点击“共享本机”
才接受连接。关闭主窗口会停止共享、关闭会话窗口并退出应用，不保留托盘或后台进程。
需要显式启动共享时可使用 `sealdesk --host`；Linux 桌面包不支持旧的
`--service`、`--server`、`--tray` 守护进程入口。

## 许可证

SealDesk 基于 [RustDesk](https://github.com/rustdesk/rustdesk) 分叉开发，沿用其 [AGPL-3.0](LICENCE) 许可证。各依赖组件遵循其各自的许可证。
