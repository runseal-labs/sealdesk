#!/usr/bin/env bash

MODE=${MODE:=release}

# Native libraries are staged by flutter/ndk_*.sh. Remove the old RustDesk
# name if a previous checkout left it in the ignored jniLibs directory.
find android/app/src/main/jniLibs -name librustdesk.so -delete
for abi_dir in android/app/src/main/jniLibs/*; do
    [ -d "${abi_dir}" ] || continue
    for library in libsealdesk.so libtailcat.so libc++_shared.so; do
        [ -f "${abi_dir}/${library}" ] || {
            echo "ERROR: missing ${abi_dir}/${library}; run the matching flutter/ndk_*.sh first" >&2
            exit 1
        }
    done
done

if [ -z "${ANDROID_SIGNING_KEY:-}" ]; then
    export SEALDESK_ANDROID_ALLOW_UNSIGNED=1
    echo "ANDROID_SIGNING_KEY is empty; allowing an unsigned release APK"
else
    export SEALDESK_ANDROID_ALLOW_UNSIGNED=0
fi

flutter build apk --target-platform android-arm64,android-arm --${MODE} --obfuscate --split-debug-info ./split-debug-info
flutter build apk --split-per-abi --target-platform android-arm64,android-arm --${MODE} --obfuscate --split-debug-info ./split-debug-info
flutter build appbundle --target-platform  android-arm64,android-arm --${MODE} --obfuscate --split-debug-info ./split-debug-info

# build in linux
# $ANDROID_NDK/toolchains/aarch64-linux-android-4.9/prebuilt/linux-x86_64/bin/aarch64-linux-android-strip android/app/src/main/jniLibs/arm64-v8a/*
