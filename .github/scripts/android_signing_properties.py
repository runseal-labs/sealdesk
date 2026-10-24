#!/usr/bin/env python3
"""Write Gradle key.properties. Secret values are read from the environment and never printed."""

import os
import sys


def encode_properties_value(value: str) -> str:
    out = []
    for ch in value:
        code = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ch in " =:#!":
            out.append("\\" + ch)
        elif code < 32 or code > 126:
            if code > 0xFFFF:
                # Java Properties.load() reads exactly four hex digits per \\u.
                encoded = ch.encode("utf-16-be")
                out.append(
                    "".join(
                        "\\u%04x" % int.from_bytes(encoded[i : i + 2], "big")
                        for i in range(0, len(encoded), 2)
                    )
                )
            else:
                out.append("\\u%04x" % code)
        else:
            out.append(ch)
    return "".join(out)


def main() -> None:
    if len(sys.argv) != 3:
        print(
            "usage: android_signing_properties.py PROPS_PATH KEYSTORE_PATH",
            file=sys.stderr,
        )
        sys.exit(2)
    props_path, keystore_path = sys.argv[1], sys.argv[2]
    env_for_key = (
        ("storePassword", "ANDROID_KEY_STORE_PASSWORD"),
        ("keyPassword", "ANDROID_KEY_PASSWORD"),
        ("keyAlias", "ANDROID_ALIAS"),
    )
    values = {}
    for prop_key, env_name in env_for_key:
        value = os.environ.get(env_name, "")
        if value == "":
            print("Android signing secrets are incomplete", file=sys.stderr)
            sys.exit(1)
        values[prop_key] = value
    if not os.path.isfile(keystore_path) or os.path.getsize(keystore_path) == 0:
        print("Android keystore is missing", file=sys.stderr)
        sys.exit(1)
    lines = [
        "%s=%s" % (prop_key, encode_properties_value(values[prop_key]))
        for prop_key, _env_name in env_for_key
    ]
    lines.append(
        "storeFile=%s" % encode_properties_value(os.path.abspath(keystore_path))
    )
    data = ("\n".join(lines) + "\n").encode("ascii")
    fd = os.open(props_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        os.write(fd, data)
    finally:
        os.close(fd)
    os.chmod(props_path, 0o600)


if __name__ == "__main__":
    main()
