#!/usr/bin/env python3
"""Build + deploy diskhound.exe (isolated staging, checked cargo/mt, kill+wait, hash)."""
import os, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import (REPO, fail, info, kill_matching, lock, ps, sha256, unlock,
                  wait_gone, wsl_path)

STAGE_WIN = r"C:\Tools\dh-build-diskhound"
STAGE = wsl_path(STAGE_WIN)
DST_WIN = r"C:\Tools\diskhound\diskhound.exe"
DST = wsl_path(DST_WIN)
EXE = os.path.join(STAGE, "target", "release", "diskhound.exe")
MANIFEST = os.path.join(REPO, "diskhound.exe.manifest")


def main():
    launch = "--launch" in sys.argv
    lock()
    try:
        ignore = shutil.ignore_patterns("target", ".git", "__pycache__", "node_modules")
        if os.path.exists(STAGE):
            shutil.rmtree(STAGE)
        shutil.copytree(REPO, STAGE, ignore=ignore)

        out = ps(f"Set-Location '{STAGE_WIN}'; cargo build --release --bin diskhound "
                 "2>&1 | Select-Object -Last 6; exit $LASTEXITCODE")
        print(out)
        if not os.path.exists(EXE):
            fail("cargo did not produce diskhound.exe")

        killed = kill_matching("diskhound", "diskhound.exe")
        info(f"killed: {killed or 'none'}")
        if not wait_gone("diskhound", timeout=15):
            fail("diskhound.exe still running — close it first")

        tmp = DST + ".new"
        shutil.copy2(EXE, tmp)
        if sha256(tmp) != sha256(EXE):
            fail("hash mismatch after staging")
        os.replace(tmp, DST)
        shutil.copy2(MANIFEST, os.path.join("/mnt/c/Tools/diskhound", "diskhound.exe.manifest"))

        mt = ps("Get-ChildItem 'C:\\Program Files (x86)\\Windows Kits\\10\\bin' -Recurse "
                "-Filter mt.exe -ErrorAction SilentlyContinue | "
                "Where-Object {$_.FullName -like '*x64*'} | Select-Object -First 1 -ExpandProperty FullName",
                check=False)
        if mt:
            r = ps(f"& '{mt}' -nologo -manifest 'C:\\Tools\\diskhound\\diskhound.exe.manifest' "
                   f"'-outputresource:{DST_WIN};#1'; exit $LASTEXITCODE", check=False)
            if "mt.exe" not in r.lower():
                fail(f"mt.exe embed failed: {r}")
            info("manifest embedded")
        else:
            info("no mt.exe — manifest shipped loose")

        info(f"published -> {DST_WIN} (sha {sha256(DST)[:12]})")
        if launch:
            ps(f"Start-Process '{DST_WIN}'")
            info("launched")
        else:
            info("not launched (pass --launch to start it)")
    finally:
        unlock()


if __name__ == "__main__":
    main()
