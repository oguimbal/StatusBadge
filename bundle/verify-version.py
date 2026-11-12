#!/usr/bin/env python3
import pathlib
import plistlib
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parents[1]
version = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"]
with (ROOT / "bundle/Info.plist").open("rb") as file:
    plist = plistlib.load(file)
assert plist["CFBundleShortVersionString"] == version, "Cargo and bundle version differ"
assert plist["CFBundleVersion"] == version, "Bundle build version differs"
if len(sys.argv) > 1:
    assert sys.argv[1] == f"v{version}", "Tag and bundle version differ"
print(version)
