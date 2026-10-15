#!/usr/bin/env bash
set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
target=${1:?请指定 Rust Android target}
go_executable=${GO:-go}
ndk_directory=${ANDROID_NDK_HOME:?请设置 ANDROID_NDK_HOME}
case "$target" in
  aarch64-linux-android) go_arch=arm64; compiler_target=aarch64-linux-android; abi=arm64-v8a ;;
  armv7-linux-androideabi) go_arch=arm; compiler_target=armv7a-linux-androideabi; abi=armeabi-v7a ;;
  x86_64-linux-android) go_arch=amd64; compiler_target=x86_64-linux-android; abi=x86_64 ;;
  i686-linux-android) go_arch=386; compiler_target=i686-linux-android; abi=x86 ;;
  *) echo "不支持的 Android target：$target" >&2; exit 1 ;;
esac
toolchain="$ndk_directory/toolchains/llvm/prebuilt/linux-x86_64/bin"
compiler="$toolchain/${compiler_target}21-clang"
readelf="$toolchain/llvm-readelf"
if [[ ! -x "$compiler" || ! -x "$readelf" ]]; then
  echo '未找到 Linux NDK 编译器，请检查 NDK 路径与安装平台。' >&2
  exit 1
fi
output_directory=${2:-"$repository/target/tailcat-android/$target"}
mkdir -p -- "$output_directory"
output_directory=$(cd -- "$output_directory" && pwd)
export GOOS=android GOARCH="$go_arch" CGO_ENABLED=1 CC="$compiler"
if [[ "$go_arch" == arm ]]; then
  export GOARM=7
else
  unset GOARM
fi
"$go_executable" -C "$repository/libs/tailcat-ffi" build \
  -buildmode=c-shared -trimpath -o "$output_directory/libtailcat.so" .
rustc --edition 2021 --crate-type cdylib --target "$target" \
  -C "linker=$compiler" -L "native=$output_directory" -l dylib=tailcat \
  "$repository/tests/tailcat_android_link.rs" -o "$output_directory/libtailcat_link_probe.so"
"$readelf" -d "$output_directory/libtailcat_link_probe.so" > "$output_directory/link-dependencies.txt"
grep -q 'NEEDED.*\[libtailcat.so\]' "$output_directory/link-dependencies.txt"
python3 "$repository/tests/test_sealdesk_android_package.py" "$abi" "$output_directory" --readelf "$readelf"
"$readelf" -h -l "$output_directory/libtailcat.so" > "$output_directory/elf-layout.txt"
"$go_executable" -C "$repository/libs/tailcat-ffi" version > "$output_directory/toolchain.txt"
"$go_executable" -C "$repository/libs/tailcat-ffi" env GOOS GOARCH CC >> "$output_directory/toolchain.txt"
printf 'PASS Android Go shared library and Rust dynamic link: %s\n' "$target"
