Name:       sealdesk
Version:    0.1.0
Release:    0
Summary:    RPM package
License:    GPL-3.0
URL:        https://rustdesk.com
Vendor:     rustdesk <info@rustdesk.com>
Requires:   gtk3 libxcb libXfixes alsa-lib libva2 gstreamer1-plugins-base
Recommends: libayatana-appindicator-gtk3 libxdo

# https://docs.fedoraproject.org/en-US/packaging-guidelines/Scriptlets/

%description
The best open-source remote desktop client software, written in Rust.

%prep
# we have no source, so nothing here

%build
# we have no source, so nothing here

%global __python %{__python3}

%install
mkdir -p %{buildroot}/usr/bin/
mkdir -p %{buildroot}/usr/share/sealdesk/
mkdir -p %{buildroot}/usr/share/sealdesk/files/
mkdir -p %{buildroot}/usr/share/icons/hicolor/256x256/apps/
mkdir -p %{buildroot}/usr/share/icons/hicolor/scalable/apps/
install -m 755 $HBB/target/release/sealdesk %{buildroot}/usr/bin/sealdesk
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/com.runseal.sealdesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/com.runseal.sealdesk.svg
install $HBB/res/sealdesk.desktop %{buildroot}/usr/share/sealdesk/files/
install $HBB/res/sealdesk-link.desktop %{buildroot}/usr/share/sealdesk/files/

%files
/usr/bin/sealdesk
/usr/share/icons/hicolor/256x256/apps/com.runseal.sealdesk.png
/usr/share/icons/hicolor/scalable/apps/com.runseal.sealdesk.svg
/usr/share/sealdesk/files/sealdesk.desktop
/usr/share/sealdesk/files/sealdesk-link.desktop
/usr/share/sealdesk/files/__pycache__/*

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
update-desktop-database


%postun
case "$1" in
  0)
    # for uninstall
    rm /usr/share/applications/sealdesk.desktop || true
    rm /usr/share/applications/sealdesk-link.desktop || true
    update-desktop-database
  ;;
  1)
    # for upgrade
  ;;
esac
