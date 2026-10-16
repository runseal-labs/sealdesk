# SealDesk

[English](README.md) | 简体中文

SealDesk 是从 RustDesk 分叉的远程桌面工具，即用即走。打开“共享本机”，
控制端扫码连接，不需要账号，也不依赖 RustDesk 的 ID 或中继服务器。
公网连接由 Tailcat 提供加密直连或 DERP 转发；完全断公网时，也能发现并连接局域网设备。

**当前为 alpha。** 目前优先验证 Linux x64 和 Android arm64；
其他平台保留了上游代码和构建流程，尚未完成 SealDesk 整体应用验收。

## 连接方式

| 方式 | 传输与授权 |
| --- | --- |
| SealDesk 配对码 | Tailcat/WireGuard 加密，沿用会话密码或点击授权策略 |
| SealDesk 局域网发现 | 强制签名与加密传输，首次由被控端点击接受，可记住控制端密钥与权限 |
| 双向兼容 RustDesk IP 直连 | 沿用原版直连协议和密码／点击授权，**不加密，明确提示** |

兼容范围是发现和 IP 直连，不包括 RustDesk ID 注册、hbbs/hbbr、账号与地址簿同步。
连接原版 RustDesk 被控端前，需要在对方开启“允许直接 IP 访问”。

IP 直连支持 IPv4 和 IPv6，不按网段预先过滤；实际可达性仍受路由、防火墙和显式访问限制影响。
SealDesk 加密连接和 RustDesk 兼容连接共用 TCP **21118**。
发现广播使用私有 IPv4 局域网接口上的 UDP **21119**，不会因为支持 IPv6 直连，
就扩大到 IPv6、公网或 100.64.0.0/10 接口。

## 使用

1. 在被控端打开 SealDesk，点击“共享本机”。
2. 控制端扫码、导入二维码图片，或粘贴完整的 `SEALDESK:` 配对码。
3. 完全断公网时，从“局域网设备”或 Android 的“发现”标签页选择设备，由被控端接受首次请求。
4. 接受时勾选“信任此设备”，后续按保存的控制端密钥和权限自动接受；
   “撤销授权”会断开该控制端，并要求下次重新确认。

配对码与 `tc...` 地址包含访问凭据，不要把它们、对应截图或未脱敏日志发到公开 issue。
二维码过期只是控制端的导入检查，不等于服务端撤权。
需要撤销密码或地址时，分别刷新密码或重置地址；退出应用本身不代表重置地址。

Linux 安装包只提供普通桌面应用，不安装 systemd 守护服务，也不配置开机自启。
普通启动进入控制模式；只有“共享本机”开启时接受连接，关闭共享会断开被控会话。
关闭主窗口会关闭会话窗口并退出，不保留后台进程。

安装 Flutter 桌面版后，可以使用：

```bash
sealdesk --host
sealdesk --control
sealdesk --control 'SEALDESK:V:1;A:<tc-address>;P:<temporary-password>;N:<name>;;'
sealdesk --control /path/to/pairing-code.png
sealdesk --tc-reset-address
```

最后一条命令请求运行中的应用断开被控会话、撤销旧地址，并仅在原先开启共享时恢复共享。
Linux 桌面包不支持旧的 `--service`、`--server`、`--tray` 守护进程入口。

## 构建与安装

Linux 和 Android 的构建步骤、原生依赖、桥接产物及仓库外签名配置见
[BUILDING.md](BUILDING.md)。刚克隆的源码还需要匹配的 Rust/Dart 生成桥接文件，
不能直接当作完整 Flutter 构建环境。

本地构建的 Linux 包可用 `sudo apt install ./sealdesk-<version>.deb` 安装。
Android 当前测试目标是 `arm64-v8a` APK。
正式发布的产物以本仓库 [Releases](https://github.com/runseal-labs/sealdesk/releases) 为准，
不要把 RustDesk 的安装包当作 SealDesk。

## 开发检查

在已配置好依赖的构建环境中运行：

```bash
cargo test --locked --lib --features linux-pkg-config tailcat::
cargo test --locked --lib --features linux-pkg-config lan::
go -C libs/tailcat-ffi test -race ./...
cd flutter
flutter test test/sealdesk_pairing_test.dart test/sealdesk_lan_panel_test.dart
```

这些检查不代表所有平台与网络环境均已验收。另见
[贡献说明](docs/CONTRIBUTING.md)、[安全反馈](docs/SECURITY.md)、
[分叉维护](FORK.md) 和[当前产品范围](docs/sealdesk-prd.md)。

## 许可证与来源

SealDesk 使用 **GNU AGPL 第 3 版**，完整保留上游 [LICENCE](LICENCE) 文本。
RustDesk 与其他上游贡献者的版权和署名继续保留。
组件许可证及来源见 [NOTICE](NOTICE) 和 [第三方声明](THIRD_PARTY_NOTICES.md)。
SealDesk 是独立分叉，不代表 RustDesk 或 Tailscale 官方。
