import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("stage_macos_update_dmgs.sh")
VERSION = "0.5.6"


def write_dmg(directory, arch, payload):
    path = directory / f"sealdesk-{VERSION}-{arch}.dmg"
    path.write_bytes(payload)
    return path


class StageMacosUpdateDmgsTests(unittest.TestCase):
    def stage(self, signed, unsigned, dest):
        return subprocess.run(
            ["bash", str(SCRIPT), VERSION, str(signed), str(unsigned), str(dest)],
            text=True,
            capture_output=True,
        )

    def test_unsigned_dmg_is_used_when_os_signing_is_absent(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            signed = root / "signed"
            unsigned = root / "unsigned"
            dest = root / "artifacts"
            signed.mkdir()
            unsigned.mkdir()
            payloads = {}
            for arch in ("aarch64", "x86_64"):
                payloads[arch] = f"unsigned-{arch}".encode()
                write_dmg(unsigned, arch, payloads[arch])

            result = self.stage(signed, unsigned, dest)

            self.assertEqual(result.returncode, 0, result.stderr)
            for arch, payload in payloads.items():
                staged = dest / f"sealdesk-{VERSION}-{arch}.dmg"
                self.assertEqual(staged.read_bytes(), payload)

    def test_signed_dmg_wins_when_both_packages_exist(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            signed = root / "signed"
            unsigned = root / "unsigned"
            dest = root / "artifacts"
            signed.mkdir()
            unsigned.mkdir()
            for arch in ("aarch64", "x86_64"):
                write_dmg(signed, arch, f"signed-{arch}".encode())
                write_dmg(unsigned, arch, f"unsigned-{arch}".encode())

            result = self.stage(signed, unsigned, dest)

            self.assertEqual(result.returncode, 0, result.stderr)
            for arch in ("aarch64", "x86_64"):
                staged = dest / f"sealdesk-{VERSION}-{arch}.dmg"
                self.assertEqual(staged.read_bytes(), f"signed-{arch}".encode())

    def test_missing_architecture_fails(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            signed = root / "signed"
            unsigned = root / "unsigned"
            dest = root / "artifacts"
            signed.mkdir()
            unsigned.mkdir()
            write_dmg(unsigned, "aarch64", b"only-arm")

            result = self.stage(signed, unsigned, dest)

            self.assertNotEqual(result.returncode, 0)
            self.assertIn(
                f"Missing update artifact {dest / f'sealdesk-{VERSION}-x86_64.dmg'}",
                result.stderr,
            )
