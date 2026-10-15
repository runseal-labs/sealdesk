import argparse
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import zipfile


ANDROID_ABIS = {
    "arm64-v8a": (2, 183),
    "armeabi-v7a": (1, 40),
    "x86_64": (2, 62),
    "x86": (1, 3),
}


def validate_package(package, abis, readelf="readelf"):
    with zipfile.ZipFile(package) as archive, tempfile.TemporaryDirectory() as directory:
        names = archive.namelist()
        packaged_abis = {
            parts[1]
            for name in names
            if len(parts := name.split("/")) == 3
            and parts[0] == "lib"
            and parts[2].endswith(".so")
        }
        if packaged_abis != set(abis):
            raise ValueError(
                f"ABI 不匹配：需要 {sorted(set(abis))}，入包 {sorted(packaged_abis)}"
            )
        for abi in sorted(packaged_abis):
            for library in ("libsealdesk.so", "libtailcat.so"):
                member = f"lib/{abi}/{library}"
                if names.count(member) != 1:
                    raise ValueError(f"必须且只能入包一份 {member}")
                data = archive.read(member)
                if len(data) < 20 or data[:4] != b"\x7fELF" or data[5] != 1:
                    raise ValueError(f"不是 Android 小端 ELF：{member}")
                elf_type, machine = struct.unpack_from("<HH", data, 16)
                if (data[4], machine) != ANDROID_ABIS[abi] or elf_type != 3:
                    raise ValueError(f"ELF 架构或动态库类型不匹配：{member}")
                if library == "libsealdesk.so":
                    extracted = Path(directory) / library
                    extracted.write_bytes(data)
                    result = subprocess.run(
                        [readelf, "--dynamic", str(extracted)],
                        check=True,
                        capture_output=True,
                        text=True,
                        env={**os.environ, "LC_ALL": "C"},
                    )
                    if not any(
                        "(NEEDED)" in line and "[libtailcat.so]" in line
                        for line in result.stdout.splitlines()
                    ):
                        raise ValueError(f"缺少 libtailcat.so 动态依赖：{member}")


def main():
    parser = argparse.ArgumentParser(description="检查 SealDesk APK 中的原生隧道库")
    parser.add_argument("package", type=Path)
    parser.add_argument("--abi", action="append", required=True, choices=ANDROID_ABIS)
    parser.add_argument("--readelf", default="readelf")
    arguments = parser.parse_args()
    try:
        validate_package(arguments.package, arguments.abi, arguments.readelf)
    except (ValueError, OSError, zipfile.BadZipFile, subprocess.CalledProcessError) as error:
        print(f"Android 入包检查失败：{error}", file=sys.stderr)
        return 1
    print(f"PASS Android native package: {arguments.package} ({', '.join(arguments.abi)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
