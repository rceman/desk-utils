# Deployment

Authoritative source: `/home/therceman/git/diskhound` (WSL). Windows-side copies are
**generated build staging only**; `C:\Tools\diskhound` is **runtime output only**.
Never edit source on the Windows side.

## Commands (run from WSL)

| Target | Command |
|---|---|
| ScrollShot | `python3 scripts/deploy_scrollshot.py` |
| sysmon_tray | `python3 scripts/deploy_sysmon_tray.py` |
| diskhound | `python3 scripts/deploy_diskhound.py [--launch]` |

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
- **Elevated rights** for sysmon_tray CPU temp (PawnIO `open` fails with
  `0x80070005` otherwise) — use tray menu → `Restart (admin)`

## What the scripts enforce

- **Serialization:** `/tmp/diskhound-deploy.lock` (one deploy at a time)
- **Build:** `exit $LASTEXITCODE` propagation — silent cargo/mt failures fail the deploy
- **Stop/wait:** only matching processes killed (`pythonw` filtered by
  `scrollshot.py` in CommandLine — other Python GUIs untouched); waits up to
  15s for exit, fails loudly if still running (e.g. elevated sibling)
- **Publish:** stage as `.new`, sha256-verify, atomic `os.replace`
- **Activate + verify (fresh, never stale):**
  - scrollshot: NEW pythonw PID + `panel ready` appended after deploy offset
  - sysmon_tray: NEW PID by CommandLine + last `pawnio` diag line printed
    (`open hr=0x0`/`ready`/`msr=` = OK; `missing export`/`0x80070005` = check)

## CPU temp diagnostics

`%TEMP%\sysmon-pawnio.log` shows the init chain: `LoadLibrary` → exports →
`open hr=` → `load hr=` → `ready` → `msr=`. All-x64 (no bitness mismatch —
verified). `0x80070005` on `open` = needs admin.

## Rollback

`git revert <sha> && re-run the deploy script`. The `.new` staging means the
last good binary is never half-overwritten.

## Done when

- ScrollShot: fresh `panel ready` in log
- sysmon_tray: process up, `pawnio: ready`/`msr=` lines for its PID
- diskhound: exe hash matches build output
