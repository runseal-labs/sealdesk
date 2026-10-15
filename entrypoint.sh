#!/bin/sh

cd "$HOME"/sealdesk || exit 1
# shellcheck source=/dev/null
. "$HOME"/.cargo/env

argv=$*

while test $# -gt 0; do
  case "$1" in
  --release)
    mkdir -p target/release
    release=1
    shift
    ;;
  --target)
    shift
    if test $# -gt 0; then
      rustup target add "$1"
      shift
    fi
    ;;
  *)
    shift
    ;;
  esac
done

if [ -z $release ]; then
  mkdir -p target/debug
fi
set -f
#shellcheck disable=2086
VCPKG_ROOT=/vcpkg cargo build --locked $argv
