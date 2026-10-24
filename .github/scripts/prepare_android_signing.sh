#!/usr/bin/env bash
# Sourced by the Android Gradle build steps.
# When signing secrets exist, materialize a keystore and key.properties under
# $RUNNER_TEMP (mode 600) and export SEALDESK_KEY_PROPERTIES. Gradle then signs
# the release APK. When they do not, allow the existing unsigned release path.
# Secret values are never printed.

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  echo "source this script from the Android build step" >&2
  exit 1
fi

if [ -z "${ANDROID_SIGNING_KEY:-}" ]; then
  export SEALDESK_ANDROID_ALLOW_UNSIGNED=1
  echo "ANDROID_SIGNING_KEY is empty; allowing an unsigned release APK"
  return 0
fi

export SEALDESK_ANDROID_ALLOW_UNSIGNED=0

if [ -z "${RUNNER_TEMP:-}" ]; then
  echo "RUNNER_TEMP is not set" >&2
  exit 1
fi

signing_dir="${RUNNER_TEMP}/sealdesk-android-signing"
case "$signing_dir" in
  "${RUNNER_TEMP}"/*) ;;
  *)
    echo "refusing to use signing directory" >&2
    exit 1
    ;;
esac
rm -rf "$signing_dir"
mkdir -p "$signing_dir" || exit 1
chmod 700 "$signing_dir" || exit 1
keystore="${signing_dir}/release.keystore"
props="${signing_dir}/key.properties"

cleanup_android_signing() {
  rm -rf "$signing_dir"
}
trap cleanup_android_signing EXIT

if ! (set -o pipefail; printf '%s' "${ANDROID_SIGNING_KEY}" | base64 -d > "$keystore"); then
  echo "Failed to decode the Android signing key" >&2
  exit 1
fi
chmod 600 "$keystore" || exit 1
if [ ! -s "$keystore" ]; then
  echo "Decoded Android keystore is empty" >&2
  exit 1
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if ! python3 "${script_dir}/android_signing_properties.py" "$props" "$keystore"; then
  echo "Failed to write Android signing properties" >&2
  exit 1
fi

export SEALDESK_KEY_PROPERTIES="$props"
unset ANDROID_SIGNING_KEY ANDROID_ALIAS ANDROID_KEY_STORE_PASSWORD ANDROID_KEY_PASSWORD
