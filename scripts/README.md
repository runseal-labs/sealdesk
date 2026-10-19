# scripts

## android-emu.sh

Headless helper for the `sealdesk34` Android Virtual Device (API 34, x86_64, Google APIs).

Requires `ANDROID_HOME` (default `~/Android/Sdk`), `tmux`, `adb` under platform-tools, and `cwebp`.

```sh
scripts/android-emu.sh start
scripts/android-emu.sh install path/to/app.apk
scripts/android-emu.sh theme dark
scripts/android-emu.sh shot /tmp/home.webp
scripts/android-emu.sh home
scripts/android-emu.sh stop
```

The emulator runs in tmux session `sealdesk-emu` (override with `SEALDESK_EMU_TMUX`).
Create the AVD once:

```sh
sdkmanager "emulator" "system-images;android-34;google_apis;x86_64"
avdmanager create avd -n sealdesk34 -k "system-images;android-34;google_apis;x86_64" -d pixel_6
```
