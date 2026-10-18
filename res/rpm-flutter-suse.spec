Name:       sealdesk
Version:    0.3.0
Release:    0
Summary:    RPM package
License:    GPL-3.0
URL:        https://rustdesk.com
Vendor:     rustdesk <info@rustdesk.com>
Requires:   gtk3 libxcb1 libXfixes3 alsa-utils libXtst6 libva2 gstreamer-plugins-base gstreamer-plugin-pipewire
Recommends: libayatana-appindicator3-1 xdotool
Provides:   libdesktop_drop_plugin.so()(64bit), libdesktop_multi_window_plugin.so()(64bit), libfile_selector_linux_plugin.so()(64bit), libflutter_custom_cursor_plugin.so()(64bit), libflutter_linux_gtk.so()(64bit), libscreen_retriever_plugin.so()(64bit), libtray_manager_plugin.so()(64bit), liburl_launcher_linux_plugin.so()(64bit), libwindow_manager_plugin.so()(64bit), libwindow_size_plugin.so()(64bit), libtexture_rgba_renderer_plugin.so()(64bit)

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

# %global __python %{__python3}

%install

mkdir -p "%{buildroot}/usr/share/sealdesk" && cp -r ${HBB}/flutter/build/linux/x64/release/bundle/* -t "%{buildroot}/usr/share/sealdesk"
mkdir -p "%{buildroot}/usr/bin"
install -Dm 644 $HBB/res/sealdesk.desktop -t "%{buildroot}/usr/share/sealdesk/files"
install -Dm 644 $HBB/res/sealdesk-link.desktop -t "%{buildroot}/usr/share/sealdesk/files"
install -Dm 644 $HBB/res/128x128@2x.png "%{buildroot}/usr/share/icons/hicolor/256x256/apps/com.runseal.sealdesk.png"
install -Dm 644 $HBB/res/scalable.svg "%{buildroot}/usr/share/icons/hicolor/scalable/apps/com.runseal.sealdesk.svg"

%files
/usr/share/sealdesk/*
/usr/share/icons/hicolor/256x256/apps/com.runseal.sealdesk.png
/usr/share/icons/hicolor/scalable/apps/com.runseal.sealdesk.svg
/usr/share/sealdesk/files/sealdesk.desktop
/usr/share/sealdesk/files/sealdesk-link.desktop

%changelog
# let's skip this for now

%pre
# Retire service units left by older packages; fresh installs do not create a daemon.
if [ -e /etc/systemd/system/sealdesk.service ] || \
   [ -e /usr/lib/systemd/system/sealdesk.service ] || \
   [ -e /usr/lib/systemd/user/sealdesk.service ]; then
    if command -v systemctl >/dev/null 2>&1; then
        systemctl disable --now sealdesk.service >/dev/null 2>&1 || true
    fi
    rm -f /etc/systemd/system/sealdesk.service \
        /usr/lib/systemd/system/sealdesk.service \
        /usr/lib/systemd/user/sealdesk.service
    if command -v systemctl >/dev/null 2>&1; then
        systemctl daemon-reload >/dev/null 2>&1 || true
    fi
fi

%post
cp /usr/share/sealdesk/files/sealdesk.desktop /usr/share/applications/
cp /usr/share/sealdesk/files/sealdesk-link.desktop /usr/share/applications/
ln -sf /usr/share/sealdesk/sealdesk /usr/bin/sealdesk
update-desktop-database


%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/bin/sealdesk || true
    rmdir /usr/lib/sealdesk || true
    rmdir /usr/local/sealdesk || true
    rmdir /usr/share/sealdesk || true
    rm /usr/share/applications/sealdesk.desktop || true
    rm /usr/share/applications/sealdesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
    rmdir /usr/lib/sealdesk || true
    rmdir /usr/local/sealdesk || true
  ;;
esac
