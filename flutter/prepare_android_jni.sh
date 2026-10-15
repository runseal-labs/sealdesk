#!/usr/bin/env bash

set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
abi=${1:?usage: prepare_android_jni.sh <abi> <rust-target>}
rust_target=${2:?usage: prepare_android_jni.sh <abi> <rust-target>}
ndk_directory=${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}

if [[ -z "${ndk_directory}" ]]; then
  echo 'ERROR: set ANDROID_NDK_HOME or ANDROID_NDK_ROOT' >&2
  exit 1
fi

case "${abi}" in
  arm64-v8a) ndk_target=aarch64-linux-android ;;
  armeabi-v7a) ndk_target=arm-linux-androideabi ;;
  x86_64) ndk_target=x86_64-linux-android ;;
  x86) ndk_target=i686-linux-android ;;
  *) echo "ERROR: unsupported Android ABI '${abi}'" >&2; exit 1 ;;
esac

host_tag=${ANDROID_NDK_HOST_TAG:-linux-x86_64}
toolchain="${ndk_directory}/toolchains/llvm/prebuilt/${host_tag}"
source_directory="${repository}/target/${rust_target}/release"
jni_directory="${repository}/flutter/android/app/src/main/jniLibs/${abi}"

for library in libsealdesk.so libtailcat.so; do
  if [[ ! -f "${source_directory}/${library}" ]]; then
    echo "ERROR: missing native library ${source_directory}/${library}" >&2
    exit 1
  fi
done

libcxx="${toolchain}/sysroot/usr/lib/${ndk_target}/libc++_shared.so"
if [[ ! -f "${libcxx}" ]]; then
  echo "ERROR: missing NDK C++ runtime ${libcxx}" >&2
  exit 1
fi

mkdir -p "${jni_directory}"
rm -f "${jni_directory}/librustdesk.so"
cp "${source_directory}/libsealdesk.so" "${jni_directory}/libsealdesk.so"
cp "${source_directory}/libtailcat.so" "${jni_directory}/libtailcat.so"
cp "${libcxx}" "${jni_directory}/libc++_shared.so"

"${toolchain}/bin/llvm-strip" "${jni_directory}/libsealdesk.so" "${jni_directory}/libtailcat.so"

printf 'Prepared Android JNI libraries for %s in %s\n' "${abi}" "${jni_directory}"
