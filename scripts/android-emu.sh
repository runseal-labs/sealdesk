#!/bin/sh
# Headless Android emulator helpers for SealDesk UI checks.
# AVD name: sealdesk34 (create it once with sdkmanager + avdmanager).
set -eu

AVD="${SEALDESK_AVD:-sealdesk34}"
TMUX_SESSION="${SEALDESK_EMU_TMUX:-sealdesk-emu}"
PKG="${SEALDESK_PKG:-com.runseal.sealdesk}"
ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export ANDROID_HOME ANDROID_SDK_ROOT="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"

EMU="$ANDROID_HOME/emulator/emulator"
ADB="$ANDROID_HOME/platform-tools/adb"

die() { printf '%s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing command: $1"
}

adb_sh() {
  "$ADB" shell "$@" | tr -d '\r'
}

cmd_start() {
  need tmux
  [ -x "$EMU" ] || die "emulator not found at $EMU"
  if tmux has-session -t "$TMUX_SESSION" 2>/dev/null; then
    echo "already running (tmux $TMUX_SESSION)"
    return 0
  fi
  tmux new-session -d -s "$TMUX_SESSION" \
    "exec \"$EMU\" -avd \"$AVD\" -no-window -gpu swiftshader_indirect -no-audio -no-boot-anim"
  echo "started $AVD in tmux session $TMUX_SESSION"
}

cmd_stop() {
  need tmux
  if "$ADB" emu kill >/dev/null 2>&1; then
    echo "sent emu kill"
  fi
  tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true
  echo "stopped"
}

cmd_install() {
  apk=${1:-}
  [ -n "$apk" ] || die "usage: install <apk>"
  [ -f "$apk" ] || die "apk not found: $apk"
  "$ADB" install -r "$apk"
}

cmd_shot() {
  out=${1:-}
  [ -n "$out" ] || die "usage: shot <out.webp>"
  need cwebp
  tmp=$(mktemp /tmp/sealdesk-shot.XXXXXX.png)
  trap 'rm -f "$tmp"' EXIT
  "$ADB" exec-out screencap -p >"$tmp"
  [ -s "$tmp" ] || die "screencap produced an empty file"
  cwebp -quiet -q 85 "$tmp" -o "$out"
  echo "$out"
}

cmd_theme() {
  mode=${1:-}
  case "$mode" in
    light) "$ADB" shell cmd uimode night no >/dev/null ;;
    dark) "$ADB" shell cmd uimode night yes >/dev/null ;;
    *) die "usage: theme light|dark" ;;
  esac
  sleep 1
  echo "theme=$mode"
}

cmd_home() {
  "$ADB" shell input keyevent KEYCODE_HOME
  echo "home"
}

usage() {
  cat <<EOF
usage: android-emu.sh <start|stop|install|shot|theme|home> [args]
  start              boot $AVD headless in tmux ($TMUX_SESSION)
  stop               kill the emulator and the tmux session
  install <apk>      adb install -r
  shot <out.webp>    screencap, convert with cwebp
  theme light|dark   toggle night mode
  home               KEYCODE_HOME
EOF
}

main() {
  sub=${1:-}
  shift || true
  case "$sub" in
    start) cmd_start ;;
    stop) cmd_stop ;;
    install) cmd_install "${1:-}" ;;
    shot) cmd_shot "${1:-}" ;;
    theme) cmd_theme "${1:-}" ;;
    home) cmd_home ;;
    *) usage; exit 2 ;;
  esac
}

main "$@"
