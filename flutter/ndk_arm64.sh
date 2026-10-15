#!/usr/bin/env bash
cargo ndk --platform 21 --target aarch64-linux-android build --locked --release --features flutter,hwcodec
bash "$(dirname -- "${BASH_SOURCE[0]}")/prepare_android_jni.sh" arm64-v8a aarch64-linux-android
