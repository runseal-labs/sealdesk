# SealDesk

[English](README.md) | 简体中文

**无需自建中继、零配置、端到端加密。扫码即连。**

在想被远程的电脑上打开 SealDesk，点「共享此设备」，用手机或另一台电脑扫一下二维码，就连上了。

不用注册账号，不用 ID 服务器，不用自建中继，也不用做端口映射。

> **Alpha 阶段。** 目前只验证了 Linux x64 和 Android arm64，其他平台还保留着 RustDesk 上游的代码，暂时不能用。

## 原理

- **扫码即连。** 二维码里装着连接需要的一切：地址、密钥和临时密码。不用手输，也不用配置。
- **端到端加密。** 所有会话都跑在 WireGuard 里，密钥只存在你自己的两台设备上。
- **不用自建中继。** 能打洞就直连；打不通时由公共 DERP 节点转发数据包，而它看不到内容。你不需要部署或配置任何服务器。
- **没网也能用。** 同一局域网内的设备会自动互相发现，断网也能连。第一次连接在被控端点同意，勾选「信任此设备」，以后直接连。

## 快速开始

1. 两台设备都装上 SealDesk（[Releases](https://github.com/runseal-labs/sealdesk/releases)）。
2. 被控端：点「共享此设备」。
3. 控制端：扫二维码，或粘贴 `SEALDESK:` 开头的配对码。

用完停止共享，所有会话随之断开，后台不会留下任何进程。

### 命令行（Linux）

```bash
sealdesk --host                     # 开始共享
sealdesk --control 'SEALDESK:...'   # 用配对码连接
sealdesk --control code.png         # 或用二维码图片
sealdesk --tc-reset-address         # 作废当前地址
```

## 保管好配对码

配对码就是你电脑的钥匙。别公开发布、别截图外传，也别贴进 issue。要作废它，就刷新密码或重置地址；光退出程序并不会让它失效。

## 和 RustDesk 互通

SealDesk 可以通过 IP 直连（TCP 21118）连接原版 RustDesk，反过来也行。这条路走的是 RustDesk 的旧协议，**不加密**，连接前 SealDesk 会提醒你。不支持 RustDesk ID、hbbs/hbbr 和账号。

## 构建

见 [BUILDING.md](BUILDING.md)。欢迎贡献，提交前请先读 [CONTRIBUTING](docs/CONTRIBUTING.md) 和 [SECURITY](docs/SECURITY.md)。

## 许可证

与上游一致，采用 AGPL-3.0。SealDesk 是 [RustDesk](https://github.com/rustdesk/rustdesk) 的独立分支，与 RustDesk、Tailscale 均无隶属或背书关系。详见 [NOTICE](NOTICE)、[第三方声明](THIRD_PARTY_NOTICES.md) 和 [FORK.md](FORK.md)。
