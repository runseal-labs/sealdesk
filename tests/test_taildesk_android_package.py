import argparse
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

from check_taildesk_android_package import ANDROID_ABIS, validate_package


class AndroidPackageTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.package = Path(self.directory.name) / "native-library-fixture.zip"
        self.rust_library = (arguments.directory / "libtailcat_link_probe.so").read_bytes()
        self.go_library = (arguments.directory / "libtailcat.so").read_bytes()

    def write_package(self, rust_library, go_library):
        with zipfile.ZipFile(self.package, "w") as archive:
            archive.writestr(f"lib/{arguments.abi}/librustdesk.so", rust_library)
            if go_library is not None:
                archive.writestr(f"lib/{arguments.abi}/libtailcat.so", go_library)

    def validate(self):
        validate_package(self.package, [arguments.abi], arguments.readelf)

    def test_real_android_libraries(self):
        self.write_package(self.rust_library, self.go_library)
        self.validate()
        other_abi = next(abi for abi in ANDROID_ABIS if abi != arguments.abi)
        with self.assertRaisesRegex(ValueError, "ABI 不匹配"):
            validate_package(self.package, [other_abi], arguments.readelf)

    def test_missing_go_library(self):
        self.write_package(self.rust_library, None)
        with self.assertRaisesRegex(ValueError, "必须且只能入包一份"):
            self.validate()

    def test_wrong_library(self):
        with self.subTest("architecture"):
            wrong_architecture = bytearray(self.go_library)
            struct.pack_into("<H", wrong_architecture, 18, 0)
            self.write_package(self.rust_library, wrong_architecture)
            with self.assertRaisesRegex(ValueError, "ELF 架构"):
                self.validate()
        with self.subTest("dynamic dependency"):
            self.write_package(self.go_library, self.go_library)
            with self.assertRaisesRegex(ValueError, "缺少 libtailcat.so 动态依赖"):
                self.validate()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="使用真实 Android 库测试 APK 原生库检查")
    parser.add_argument("abi", choices=ANDROID_ABIS)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--readelf", default="readelf")
    arguments = parser.parse_args()
    unittest.main(argv=[__file__], verbosity=2)
