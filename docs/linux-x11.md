# Linux: X11 required

SealDesk shares the screen through X11. If the host is logged in to a Wayland
session, the controller may see this message instead of the desktop.

To fix it, log out on the host and pick an **Xorg / X11** session on the login
screen (usually behind the gear icon), then log back in and share again.

On GDM you can make X11 the default by setting `WaylandEnable=false` in
`/etc/gdm3/custom.conf` and restarting.

---

# Linux：需要 X11

SealDesk 通过 X11 共享屏幕。如果被控端登录的是 Wayland 会话，控制端可能会看到这条提示，而不是桌面。

解决办法：在被控端注销，在登录界面（一般在齿轮图标里）选择 **Xorg / X11** 会话，重新登录后再共享。

使用 GDM 的话，可以在 `/etc/gdm3/custom.conf` 里设置 `WaylandEnable=false` 并重启，把 X11 设为默认。
