import base64
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
PROPERTIES_SCRIPT = SCRIPT_DIR / "android_signing_properties.py"
PREPARE_SCRIPT = SCRIPT_DIR / "prepare_android_signing.sh"
WORKFLOW = SCRIPT_DIR.parents[1] / ".github/workflows/flutter-build.yml"
PASSWORD_FALLBACK = (
    "ANDROID_KEY_PASSWORD: ${{ secrets.ANDROID_KEY_PASSWORD || secrets.ANDROID_KEY_STORE_PASSWORD }}"
)


class AndroidSigningPropertiesTests(unittest.TestCase):
    def write_properties(self, env_updates):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        root = Path(directory.name)
        keystore = root / "release.keystore"
        keystore.write_bytes(b"keystore")
        props = root / "key.properties"
        env = os.environ.copy()
        for name in (
            "ANDROID_KEY_STORE_PASSWORD",
            "ANDROID_KEY_PASSWORD",
            "ANDROID_ALIAS",
        ):
            env.pop(name, None)
        env.update(env_updates)
        result = subprocess.run(
            [sys.executable, str(PROPERTIES_SCRIPT), str(props), str(keystore)],
            env=env,
            capture_output=True,
            check=False,
        )
        return result, props

    def assert_secret_not_printed(self, result, *secrets):
        output = result.stdout + result.stderr
        for secret in secrets:
            if secret:
                self.assertNotIn(secret.encode(), output)

    def test_empty_key_password_uses_store_password(self):
        store_password = "store-secret"
        result, props = self.write_properties(
            {
                "ANDROID_KEY_STORE_PASSWORD": store_password,
                "ANDROID_KEY_PASSWORD": "",
                "ANDROID_ALIAS": "release-alias",
            }
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_secret_not_printed(result, store_password, "release-alias")
        lines = props.read_text(encoding="ascii").splitlines()
        self.assertEqual(lines[0], "storePassword=%s" % store_password)
        self.assertEqual(lines[1], "keyPassword=%s" % store_password)

    def test_missing_key_password_uses_store_password(self):
        store_password = "store-secret"
        result, props = self.write_properties(
            {
                "ANDROID_KEY_STORE_PASSWORD": store_password,
                "ANDROID_ALIAS": "release-alias",
            }
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_secret_not_printed(result, store_password)
        self.assertIn("keyPassword=%s" % store_password, props.read_text(encoding="ascii"))

    def test_distinct_key_password_is_kept(self):
        key_password = "key-secret"
        result, props = self.write_properties(
            {
                "ANDROID_KEY_STORE_PASSWORD": "store-secret",
                "ANDROID_KEY_PASSWORD": key_password,
                "ANDROID_ALIAS": "release-alias",
            }
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_secret_not_printed(result, "store-secret", key_password)
        self.assertIn("keyPassword=%s" % key_password, props.read_text(encoding="ascii"))

    def test_incomplete_secrets_do_not_print_values(self):
        alias = "release-alias"
        result, props = self.write_properties({"ANDROID_ALIAS": alias})
        self.assertEqual(result.returncode, 1)
        self.assertEqual(result.stderr, b"Android signing secrets are incomplete\n")
        self.assertEqual(result.stdout, b"")
        self.assert_secret_not_printed(result, alias)
        self.assertFalse(props.exists())

    def test_prepare_script_falls_back_without_printing(self):
        store_password = "store-secret"
        alias = "release-alias"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            copied = root / "copied.properties"
            env = os.environ.copy()
            for name in (
                "ANDROID_KEY_STORE_PASSWORD",
                "ANDROID_KEY_PASSWORD",
                "ANDROID_ALIAS",
                "ANDROID_SIGNING_KEY",
                "SEALDESK_KEY_PROPERTIES",
                "SEALDESK_ANDROID_ALLOW_UNSIGNED",
            ):
                env.pop(name, None)
            env.update(
                {
                    "RUNNER_TEMP": str(root),
                    "ANDROID_SIGNING_KEY": base64.b64encode(b"keystore").decode("ascii"),
                    "ANDROID_KEY_STORE_PASSWORD": store_password,
                    "ANDROID_ALIAS": alias,
                }
            )
            command = "\n".join(
                (
                    "source %s" % PREPARE_SCRIPT,
                    'cp "$SEALDESK_KEY_PROPERTIES" %s' % copied,
                )
            )
            result = subprocess.run(
                ["bash", "-c", command],
                env=env,
                capture_output=True,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assert_secret_not_printed(result, store_password, alias)
            text = copied.read_text(encoding="ascii")
        self.assertIn("keyPassword=%s" % store_password, text)
        self.assertNotIn(store_password, result.stdout.decode())

    def test_both_android_jobs_fall_back_in_the_workflow(self):
        text = WORKFLOW.read_text(encoding="utf-8")
        self.assertEqual(text.count(PASSWORD_FALLBACK), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
