# Deployment

Authoritative source: `/home/therceman/git/diskhound` (WSL). Windows-side copies are
**generated build staging only**; `C:\Tools\diskhound` is **runtime output only**.
Never edit source on the Windows side.

## Commands (run from WSL)

| Target | Command |
|---|---|
| ScrollShot | `python3 scripts/deploy_scrollshot.py` |
| sysmon_tray | `python3 scripts/deploy_sysmon_tray.py [--elevated]` |
| diskhound | `python3 scripts/deploy_diskhound.py [--launch]` |

`--elevated` relaunches the tray via UAC after publish (required for CPU temp —
PawnIO `open` returns `0x80070005` without admin). The script also detects a
non-elevated failure and retries elevated once automatically.

## Paths

- **Source (authoritative):** `~/git/diskhound`
- **Build staging (ephemeral, per target):** `C:\Tools\dh-build-{sysmon,diskhound}`
- **Runtime output:** `C:\Tools\diskhound\`
- **ScrollShot log:** `C:\Users\therceman\Pictures\Screenshots\ScrollShot\scrollshot.log`
- **PawnIO diag:** `%TEMP%\sysmon-pawnio.log`

## Prerequisites

- `powershell.exe` reachable from WSL; `cargo`/`rustc` on Windows PATH
- Python at `C:\Program Files\Python312\pythonw.exe` (+ mss/Pillow/numpy)
- PawnIO installed (`C:\Program Files\PawnIO\PawnIOLib.dll`) for CPU temp
- Admin rights for elevated tray mode (UAC prompt appears)

## What the scripts enforce

- **Serialization:** `/tmp/diskhound-deploy.lock` — one deploy at a time
- **Build:** `exit $LASTEXITCODE` propagation; cargo/mt failure aborts before publish
- **Manifest:** embedded into the *staged* exe; a mt failure never touches the live binary
- **Stop/wait:** only processes whose CommandLine contains the deployed path are
  killed (e.g. `diskhound\sysmon_tray.exe`, `scrollshot.py` under the tools path);
  other checkouts and unrelated pythonw GUIs are untouched. Waits up to 15s for
  exit and fails loudly if a process survives (e.g. an elevated sibling —
  kill it via Task Manager or an elevated taskkill)
- **Publish:** stage as `.new` → sha256-verify → atomic `os.replace`; on failure
  the previous binary stays untouched
- **Activate + verify (fresh, never stale):**
  - scrollshot: NEW pythonw PID + `panel ready` appended after deploy offset
  - sysmon_tray: NEW PID by CommandLine + `pawnio` diag checked for its PID —
    `ready`/`msr=` = OK; `0x80070005` = auto-retry elevated; anything else = FAIL
  - diskhound: hash-verified publish; `--launch` starts it explicitly
- **Process discovery errors raise** — a failed WMI/JSON read never becomes
  "no processes" / silent success

## CPU temp diagnostics

`%TEMP%\sysmon-pawnio.log` shows the init chain per PID: `LoadLibrary` →
`open hr=` → `load hr=` → `ready` → `msr=`. `0x80070005` = needs admin;
`missing export` = wrong binary/source; `another instance still holds the
mutex` = another tray (often elevated) already owns the singleton.

## Rollback

`git revert <sha>` and re-run the deploy script. The `.new` staging step means a
failed publish never leaves a half-written binary; the last good exe stays live.

## Done when

- ScrollShot: fresh `panel ready` in log for the new PID
- sysmon_tray: process up + `ready`/`msr=` pawnio lines for its PID
- diskhound: deployed hash matches build output
