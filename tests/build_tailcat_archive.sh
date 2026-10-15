#!/usr/bin/env bash
set -euo pipefail

repository=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
go_executable=${GO:-go}
compiler=${TAILCAT_SMOKE_CC:-cc}
archive_directory=${1:-"$repository/target/tailcat-archive"}
go_os=$("$go_executable" env GOOS)
go_arch=$("$go_executable" env GOARCH)
if [[ "$go_os" != "$("$go_executable" env GOHOSTOS)" ||
      "$go_arch" != "$("$go_executable" env GOHOSTARCH)" ]]; then
  echo '此脚本需要运行本机架构的链接探针；交叉编译请使用目标平台任务。' >&2
  exit 1
fi

link_flags=()
executable_suffix=
case "$go_os" in
  linux) link_flags=(-pthread -ldl -lm) ;;
  darwin) link_flags=(-framework CoreFoundation -framework Security -lresolv) ;;
  windows)
    link_flags=(-lwinmm -lntdll -lws2_32)
    executable_suffix=.exe
    ;;
  *)
    echo "本机静态库探针尚不支持平台：$go_os" >&2
    exit 1
    ;;
esac

mkdir -p -- "$archive_directory"
archive_directory=$(cd -- "$archive_directory" && pwd)
export CGO_ENABLED=1
"$go_executable" -C "$repository/libs/tailcat-ffi" test -race -count=1 ./...
"$go_executable" -C "$repository/libs/tailcat-ffi" build \
  -buildmode=c-archive -trimpath -o "$archive_directory/libtailcat.a" .
"$compiler" -std=c11 -Wall -Wextra -Werror \
  -I "$archive_directory" "$repository/tests/tailcat_archive_smoke.c" \
  "$archive_directory/libtailcat.a" "${link_flags[@]}" \
  -o "$archive_directory/tailcat-smoke$executable_suffix"
"$archive_directory/tailcat-smoke$executable_suffix"
"$go_executable" -C "$repository/libs/tailcat-ffi" version > "$archive_directory/toolchain.txt"
"$go_executable" -C "$repository/libs/tailcat-ffi" env GOOS GOARCH CC CGO_ENABLED >> "$archive_directory/toolchain.txt"
