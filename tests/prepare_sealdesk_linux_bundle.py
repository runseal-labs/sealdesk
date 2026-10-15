import argparse
import ctypes
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


def prepare_bundle(bundle, library_directories, patchelf):
    core = bundle / "lib" / "libsealdesk.so"
    if not core.is_file():
        raise ValueError(f"找不到原生库：{core}")
    if shutil.which(patchelf) is None:
        raise ValueError("找不到 patchelf，请安装构建工具或使用 --patchelf 指定路径")
    environment = dict(os.environ, LC_ALL="C")
    environment.pop("LD_LIBRARY_PATH", None)
    environment.pop("LD_PRELOAD", None)
    subprocess.run([patchelf, "--set-rpath", "$ORIGIN", str(core)], check=True)
    copied = set()
    while True:
        result = subprocess.run(
            ["ldd", str(core)], capture_output=True, text=True, env=environment
        )
        missing = set(re.findall(r"^\s*(\S+)\s+=>\s+not found\s*$", result.stdout, re.M))
        if not missing:
            if result.returncode != 0:
                raise ValueError(result.stdout + result.stderr)
            break
        for name in sorted(missing):
            if Path(name).name != name or name in copied:
                raise ValueError(f"无法解析动态库：{name}")
            source = next(
                (directory / name for directory in library_directories
                 if (directory / name).is_file()),
                None,
            )
            if source is None:
                raise ValueError(f"缺少匹配的 {name}，请用 --library-dir 指定原构建环境的库目录")
            destination = core.parent / name
            shutil.copyfile(source, destination)
            subprocess.run(
                [patchelf, "--set-rpath", "$ORIGIN", str(destination)], check=True
            )
            copied.add(name)
            print(f"Bundled {name}")
    subprocess.run(
        [sys.executable, "-c", "import ctypes, sys; ctypes.CDLL(sys.argv[1])", str(core)],
        check=True,
        env=environment,
    )
    print(f"PASS Linux native dependencies and dlopen: {core}")


def main():
    parser = argparse.ArgumentParser(description="为本机 Linux 测试包补齐原构建环境的动态库")
    parser.add_argument("bundle", type=Path)
    parser.add_argument("--library-dir", type=Path, action="append", required=True)
    parser.add_argument("--patchelf", default="patchelf")
    arguments = parser.parse_args()
    try:
        prepare_bundle(arguments.bundle.resolve(), arguments.library_dir, arguments.patchelf)
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f"Linux 测试包准备失败：{error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
