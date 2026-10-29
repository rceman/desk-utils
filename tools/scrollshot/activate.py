"""Deploy scrollshot.py from repo to C:\\Tools\\diskhound\\tools\\scrollshot\\.

Usage:  python activate.py            -> copy deployed script over the repo target dir
        python activate.py --restart  -> also restart the running ScrollShot app
"""
import argparse, os, shutil, subprocess, sys

REPO = os.path.dirname(os.path.abspath(__file__))
DEPLOY = r"C:\Tools\diskhound\tools\scrollshot"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--restart", action="store_true")
    args = ap.parse_args()

    src = os.path.join(REPO, "scrollshot.py")
    dst = os.path.join(DEPLOY, "scrollshot.py")
    os.makedirs(DEPLOY, exist_ok=True)
    shutil.copy2(src, dst)
    print(f"deployed: {dst}")

    if args.restart:
        subprocess.run(["powershell", "-NoProfile", "-Command",
                        "Get-Process pythonw -ErrorAction SilentlyContinue | Stop-Process -Force"],
                       check=False)
        subprocess.Popen(["pythonw", dst])
        print("restarted")


if __name__ == "__main__":
    main()
