#!/usr/bin/env bash
# Copy the macOS update DMGs into DEST.
# The OS-signed artifact is used when it was produced. Otherwise the ad-hoc
# DMG is used. A successful macOS build must still provide both architectures.
set -euo pipefail

if [ "$#" -ne 4 ]; then
  echo "usage: stage_macos_update_dmgs.sh VERSION SIGNED_DIR UNSIGNED_DIR DEST_DIR" >&2
  exit 2
fi

version=$1
signed_dir=$2
unsigned_dir=$3
dest_dir=$4

mkdir -p "$dest_dir"

for arch in aarch64 x86_64; do
  name="sealdesk-${version}-${arch}.dmg"
  dest="${dest_dir%/}/${name}"
  signed="${signed_dir%/}/${name}"
  unsigned="${unsigned_dir%/}/${name}"
  if [ -f "$signed" ]; then
    cp "$signed" "$dest"
  elif [ -f "$unsigned" ]; then
    cp "$unsigned" "$dest"
  else
    echo "::error::Missing update artifact ${dest}" >&2
    exit 1
  fi
done
