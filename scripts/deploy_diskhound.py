#!/usr/bin/env python3
"""Build + deploy diskhound.exe (cargo on Windows-local copy + manifest)."""
import os, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import REPO, TOOLS, info, sh

BUILD_SRC = os.path.join(TOOLS, "dh-src")
BUILD_SRC_WIN = r"C:\Tools\dh-src"
EXE = os.path.join(BUILD_SRC, "target", "release", "diskhound.exe")
DST_WIN = r"C:\Tools\diskhound\diskhound.exe"
MANIFEST = os.path.join(REPO, "diskhound.exe.manifest")
MANIFEST_WIN = r"C:\Tools\diskhound\diskhound.exe.manifest"


def main():
    ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
    if os.path.exists(BUILD_SRC):
        shutil.rmtree(BUILD_SRC)
    shutil.copytree(REPO, BUILD_SRC, ignore=ignore)
    info(f"synced repo -> {BUILD_SRC}")

    out = sh(f"Set-Location '{BUILD_SRC_WIN}'; cargo build --release --bin diskhound 2>&1 | Select-Object -Last 4")
    print(out)
    if "Finished" not in out or not os.path.exists(EXE):
        sys.exit("[deploy] FAIL: build did not produce diskhound.exe")

    sh(f"Copy-Item '{os.path.join(BUILD_SRC_WIN, r'target\release\diskhound.exe')}' '{DST_WIN}' -Force", check=True)
    shutil.copy2(MANIFEST, os.path.join("/mnt/c/Tools/diskhound", "diskhound.exe.manifest"))
    # embed manifest if mt.exe is available
    mt = sh("Get-ChildItem 'C:\\Program Files (x86)\\Windows Kits\\10\\bin' -Recurse -Filter mt.exe "
            "-ErrorAction SilentlyContinue | Where-Object {$_.FullName -like '*x64*'} | "
            "Select-Object -First 1 -ExpandProperty FullName")
    if mt:
        sh(f"& '{mt}' -nologo -manifest '{MANIFEST_WIN}' '-outputresource:{DST_WIN};#1'")
        info("manifest embedded")
    else:
        info("manifest shipped loose")
    info(f"deployed -> {DST_WIN} (not restarted — run manually when needed)")


if __name__ == "__main__":
    main()
