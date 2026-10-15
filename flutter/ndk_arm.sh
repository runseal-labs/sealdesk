#!/usr/bin/env bash
cargo ndk --platform 21 --target armv7-linux-androideabi build --locked --release --features flutter,hwcodec
bash "$(dirname -- "${BASH_SOURCE[0]}")/prepare_android_jni.sh" armeabi-v7a armv7-linux-androideabi
