<p align="center">
  <img src="res/icon.png" width="160" alt="SealDesk 海豹抱猫图标">
</p>

# SealDesk

[English](README.md) | 简体中文

**无需自建中继、零配置、端到端加密。扫码即连。**

在想被远程的电脑上打开 SealDesk，点「共享此设备」，用手机或另一台电脑扫一下二维码，就连上了。

不用注册账号，不用 ID 服务器，不用自建中继，也不用做端口映射。

> 已在 Linux x64 / aarch64、Windows 和 macOS 上验证：作为被控端和控制端连接均正常。Android 目前主要作为控制端使用。
>
> **macOS**
>
> - 应用未公证。首次打开前可执行：
>
>   ```bash
>   xattr -r -d com.apple.quarantine /Applications/SealDesk.app
>   ```
>
> - 应用是 adhoc 签名。每次升级后，屏幕录制、辅助功能、输入监视权限可能失效。在**系统设置 > 隐私与安全性**中移除旧的 SealDesk 条目后重新授权。
> - 屏幕录制授权后可能需要退出并重新打开应用。
>
> 本版本不包含 iOS 版本。

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
sealdesk --control -                # 从标准输入读取配对码
sealdesk --control-file ./code.txt  # Linux/macOS：0600 且属于当前用户的文件
sealdesk --control code.png         # 或用二维码图片（路径里不要放密码）
# --control 会读取 SEALDESK_CONTROL_CODE，读完即从环境中清除。
# 该变量若已设置，优先于命令行里的 --control <配对码>。
# --control - 和 --control-file 只表示使用该变量，本身不是配对码。
# 含密码的配对码不能写在命令行参数里。
# Windows 不支持 --control-file，请改用 --control - 或 SEALDESK_CONTROL_CODE。
sealdesk --tc-reset-address         # 作废当前地址
```

### 中继地图与连接故障

桌面首页的「中继设置」可保存 HTTPS DERP map 地址，也可恢复默认值。
初始化失败会显示地图获取或中继引导阶段的错误，并提供「重试」。
共享时保存会重启共享并断开当前会话。配置生效时间、内嵌中继地址、TLS 校验
及网络限制见 [DERP 配置与排障说明](docs/DERP.md)。

## 连不上怎么办

- **被控端没弹同意提示**：确认被控端正处于「共享此设备」状态，且没有锁屏或休眠。
- **扫码后超时**：二维码能生成只说明被控端已就绪，不代表链路已通。有些网络（公司/校园网、带强制代理或深度拦截的宽带）会同时封掉直连与 DERP 中继，此时扫码必然超时；换到同一个局域网，或换个网络（如手机热点）再试。共享页会显示中继连通状态，可先在那里确认。
- **macOS 连上后黑屏或无法操作**：多半是升级后权限失效，按上方 macOS 说明移除旧条目并重新授权。

## 保管好配对码

配对码就是你电脑的钥匙。别公开发布、别截图外传，也别贴进 issue。要作废它，就刷新密码或重置地址；光退出程序并不会让它失效。

## 和 RustDesk 互通

SealDesk 可以通过 IP 直连（TCP 21118）连接原版 RustDesk，反过来也行。这条路走的是 RustDesk 的旧协议，**不加密**。默认关闭，打开时会给出明确警告，连接前也会提醒。不支持 RustDesk ID、hbbs/hbbr 和账号。

## 构建

见 [BUILDING.md](BUILDING.md)。欢迎贡献，提交前请先读 [CONTRIBUTING](docs/CONTRIBUTING.md) 和 [SECURITY](docs/SECURITY.md)。

## 许可证

与上游一致，采用 AGPL-3.0。SealDesk 是 [RustDesk](https://github.com/rustdesk/rustdesk) 的独立分支，与 RustDesk、Tailscale 均无隶属或背书关系。详见 [NOTICE](NOTICE)、[第三方声明](THIRD_PARTY_NOTICES.md) 和 [FORK.md](FORK.md)。
