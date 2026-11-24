"""Packs the release executables and the README into dist/rawv-windows-x64.zip.

Usage: python tools/package.py   (after cargo build --release)
"""
import sys
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FILES = [REPO / "target" / "release" / "rawv.exe", REPO / "target" / "release" / "rawv-cli.exe", REPO / "README.md"]

missing = [str(path) for path in FILES if not path.exists()]
if missing:
    sys.exit("missing: " + ", ".join(missing))
target = REPO / "dist" / "rawv-windows-x64.zip"
target.parent.mkdir(exist_ok=True)
with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
    for path in FILES:
        archive.write(path, f"rawv/{path.name}")
print(f"{target} {target.stat().st_size / 1e6:.1f} MB")
