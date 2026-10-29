"""DEPRECATED — use the canonical WSL deploy script instead:

    python3 ~/git/diskhound/scripts/deploy_scrollshot.py

This file only forwards to the real deployer so old habits keep working.
"""
import os, subprocess, sys

REPO = os.environ.get("DISKHOUND_REPO", "/home/therceman/git/diskhound")


def main():
    script = os.path.join(REPO, "scripts", "deploy_scrollshot.py")
    if os.path.exists("/mnt/c") and os.path.exists(script):
        sys.exit(subprocess.call([sys.executable, script]))
    print("activate.py is deprecated. From WSL run:")
    print("    python3 ~/git/diskhound/scripts/deploy_scrollshot.py")
    sys.exit(1)


if __name__ == "__main__":
    main()
