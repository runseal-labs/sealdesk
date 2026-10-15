#!/usr/bin/env bash
cargo ndk --platform 21 --target x86_64-linux-android build --locked --release --features flutter
bash "$(dirname -- "${BASH_SOURCE[0]}")/prepare_android_jni.sh" x86_64 x86_64-linux-android
