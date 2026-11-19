"""Path of the Windows virtual-display cdylib that build.py copies.

Cargo 1.99.1 leaves an extra copy under <profile>/deps/. Cargo 1.100.0 does
not create <profile>/deps. Both put the final cdylib in the profile directory
(measured on Linux as libdylib_virtual_display.so). The Windows script uses
the same parent and the basename it already copied from deps/.
"""

import importlib.util
import os
import shutil
import tempfile
import unittest
from pathlib import Path

_SPEC = importlib.util.spec_from_file_location(
    "sealdesk_build", Path(__file__).resolve().parents[1] / "build.py")
build = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(build)
windows_virtual_display_dll = build.windows_virtual_display_dll


class WindowsVirtualDisplayCopyTests(unittest.TestCase):
    def test_copy_uses_profile_dir_when_deps_is_absent(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root)
        profile = os.path.join(root, "target", "release")
        os.makedirs(profile)
        src = windows_virtual_display_dll(profile)
        with open(src, "wb") as handle:
            handle.write(b"dll")
        dest = os.path.join(root, "flutter-out")
        os.makedirs(dest)
        shutil.copy2(src, dest)
        self.assertTrue(os.path.isfile(os.path.join(dest, "dylib_virtual_display.dll")))
        self.assertFalse(os.path.isdir(os.path.join(profile, "deps")))

    def test_profile_file_matches_legacy_deps_basename(self):
        root = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, root)
        profile = os.path.join(root, "release")
        deps = os.path.join(profile, "deps")
        os.makedirs(deps)
        name = "dylib_virtual_display.dll"
        for directory in (profile, deps):
            with open(os.path.join(directory, name), "wb") as handle:
                handle.write(b"dll")
        chosen = windows_virtual_display_dll(profile)
        self.assertEqual(os.path.basename(chosen), name)
        self.assertNotEqual(os.path.dirname(chosen), deps)


if __name__ == "__main__":
    unittest.main()
