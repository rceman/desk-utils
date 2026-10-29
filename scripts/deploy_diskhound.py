#!/usr/bin/env python3
"""Build + deploy diskhound.exe (isolated staging, checked cargo/mt, kill+wait, hash).

Manifest is embedded into the STAGED exe before publication — a mt failure aborts
the deploy and never touches the live binary.
"""
import os, shutil, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _win import (REPO, fail, info, kill_matching, lock, ps, sha256, unlock,
                  wait_gone, wsl_path)

STAGE_WIN = r"C:\Tools\dh-build-diskhound"
STAGE = wsl_path(STAGE_WIN)
DST_WIN = r"C:\Tools\diskhound\diskhound.exe"
DST = wsl_path(DST_WIN)
MATCH = r"C:\Tools\diskhound\diskhound.exe"
EXE = os.path.join(STAGE, "target", "release", "diskhound.exe")
EXE_WIN = STAGE_WIN + r"\target\release\diskhound.exe"
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

        # embed manifest into the STAGED exe — failure aborts before publish (A1)
        stage_manifest = os.path.join(STAGE, "diskhound.exe.manifest")
        shutil.copy2(MANIFEST, stage_manifest)
        mt = ps("Get-ChildItem 'C:\\Program Files (x86)\\Windows Kits\\10\\bin' -Recurse "
                "-Filter mt.exe -ErrorAction SilentlyContinue | "
                "Where-Object {$_.FullName -like '*x64*'} | Select-Object -First 1 -ExpandProperty FullName",
                check=False)
        if mt:
            ps(f"& '{mt}' -nologo -manifest '{STAGE_WIN}\\diskhound.exe.manifest' "
               f"'-outputresource:{EXE_WIN};#1' | Out-Null; exit $LASTEXITCODE")
            info("manifest embedded into staged exe")
        else:
            info("no mt.exe — manifest ships loose next to exe")

        killed = kill_matching("diskhound", MATCH)
        info(f"killed: {killed or 'none'}")
        if not wait_gone("diskhound", MATCH, timeout=15):
            fail("diskhound.exe still running — close it first")

        tmp = DST + ".new"
        man_dst = "/mnt/c/Tools/diskhound/diskhound.exe.manifest"
        man_tmp = man_dst + ".new"
        shutil.copy2(EXE, tmp)
        # stage the sidecar BEFORE publishing anything — failure aborts cleanly
        try:
            shutil.copy2(MANIFEST, man_tmp)
        except OSError as e:
            if not mt:
                fail(f"no mt AND sidecar staging failed ({e}) — nothing published")
            man_tmp = None
            info(f"warn: loose manifest staging failed ({e}) — embedded manifest present")
        if sha256(tmp) != sha256(EXE):
            fail("hash mismatch after staging")
        bak = DST + ".bak"; man_bak = man_dst + ".bak"
        try:
            if os.path.exists(DST):
                shutil.copy2(DST, bak)
            if os.path.exists(man_dst):
                shutil.copy2(man_dst, man_bak)
            if man_tmp:
                os.replace(man_tmp, man_dst)
            os.replace(tmp, DST)
        except OSError as e:
            if os.path.exists(bak):
                shutil.copy2(bak, DST)
            if os.path.exists(man_bak):
                shutil.copy2(man_bak, man_dst)
            fail(f"publish failed ({e}) — previous release restored")
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
