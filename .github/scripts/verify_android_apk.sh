#!/usr/bin/env bash
# Confirm each APK is signed and print only certificate SHA-256 digests.
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: verify_android_apk.sh APK..." >&2
  exit 2
fi

version="${ANDROID_SIGN_TOOL_VERSION:-}"
if [ -z "$version" ]; then
  echo "ANDROID_SIGN_TOOL_VERSION is not set" >&2
  exit 1
fi

signer="/usr/local/lib/android/sdk/build-tools/${version}/apksigner"
if [ ! -e "$signer" ] && [ -n "${ANDROID_HOME:-}" ]; then
  signer="${ANDROID_HOME}/build-tools/${version}/apksigner"
fi
if [ ! -e "$signer" ]; then
  echo "apksigner not found" >&2
  exit 1
fi

for apk in "$@"; do
  if [ ! -f "$apk" ]; then
    echo "APK not found" >&2
    exit 1
  fi
  certs="$(mktemp "${RUNNER_TEMP:-/tmp}/apk-certs.XXXXXX")"
  if ! "$signer" verify --print-certs "$apk" >"$certs" 2>&1; then
    echo "APK signature verification failed" >&2
    cat "$certs" >&2
    rm -f "$certs"
    exit 1
  fi
  digests="$(grep -oE 'SHA-256 digest: [0-9a-fA-F:]+' "$certs" || true)"
  rm -f "$certs"
  if [ -z "$digests" ]; then
    echo "APK is not signed" >&2
    exit 1
  fi
  printf '%s\n' "$digests"
done
