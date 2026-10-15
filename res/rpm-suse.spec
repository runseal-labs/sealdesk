Name:       sealdesk
Version:    1.1.9
Release:    0
Summary:    RPM package
License:    GPL-3.0
Requires:   gtk3 libxcb1 libXfixes3 alsa-utils libXtst6 libva2 gstreamer-plugins-base gstreamer-plugin-pipewire
Recommends: libayatana-appindicator3-1 xdotool

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
install $HBB/libsciter-gtk.so %{buildroot}/usr/share/sealdesk/libsciter-gtk.so
install $HBB/res/sealdesk.service %{buildroot}/usr/share/sealdesk/files/
install $HBB/res/128x128@2x.png %{buildroot}/usr/share/icons/hicolor/256x256/apps/sealdesk.png
install $HBB/res/scalable.svg %{buildroot}/usr/share/icons/hicolor/scalable/apps/sealdesk.svg
install $HBB/res/sealdesk.desktop %{buildroot}/usr/share/sealdesk/files/
install $HBB/res/sealdesk-link.desktop %{buildroot}/usr/share/sealdesk/files/

%files
/usr/bin/sealdesk
/usr/share/sealdesk/libsciter-gtk.so
/usr/share/sealdesk/files/sealdesk.service
/usr/share/icons/hicolor/256x256/apps/sealdesk.png
/usr/share/icons/hicolor/scalable/apps/sealdesk.svg
/usr/share/sealdesk/files/sealdesk.desktop
/usr/share/sealdesk/files/sealdesk-link.desktop

%changelog
# let's skip this for now

%pre
# can do something for centos7
case "$1" in
  1)
    # for install
  ;;
  2)
    # for upgrade
    systemctl stop sealdesk || true
  ;;
esac

%post
cp /usr/share/sealdesk/files/sealdesk.service /etc/systemd/system/sealdesk.service
cp /usr/share/sealdesk/files/sealdesk.desktop /usr/share/applications/
cp /usr/share/sealdesk/files/sealdesk-link.desktop /usr/share/applications/
systemctl daemon-reload
systemctl enable sealdesk
systemctl start sealdesk
update-desktop-database

%preun
case "$1" in
  0)
    # for uninstall
    systemctl stop sealdesk || true
    systemctl disable sealdesk || true
    rm /etc/systemd/system/sealdesk.service || true
  ;;
  1)
    # for upgrade
  ;;
esac

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
