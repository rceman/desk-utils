# Agent references

- **Rust idioms / review:** BeautifulRust skills cloned at `~/git/BeautifulRust/skills/`
  (`rust-best-practices`, `rust-port`, `rust-review`; origin github.com/morganlinton/BeautifulRust,
  www.beautifulrust.dev). Consult before writing/reviewing Rust.
- **Sol review workflow:** `tools/scrollshot/SOL_REVIEW.md` — airelay review_sol session usage.
- **Deployment:** `DEPLOYMENT.md` — canonical scripts `scripts/deploy_{scrollshot,sysmon_tray,diskhound}.py`.
  WSL is authoritative; `C:\Tools\dh-build-*` = staging, `C:\Tools\diskhound` = runtime.
  `activate.py` is deprecated (forwards to the WSL script).
- **ScrollShot→MD:** `tools/scrollshot/SCROLL2MD.md` — sausage PNG to markdown
  pipeline (chunk with overlap → parallel vision agents → stitch).
- **ScrollShot runtime:** log at `C:\Users\therceman\Pictures\Screenshots\ScrollShot\scrollshot.log`.
  Verify UI changes via log (`start`, `panel ready`, `kb hook=`) — silent Tk callback/hook
  crashes are invisible under pythonw. Wrap hook/worker bodies in try/except → `log()`.

## Python Win32 app practices (ScrollShot)

- **WH_KEYBOARD_LL proc budget = microseconds.** No file I/O, logging, or Tk calls inside the
  hook — Windows kills slow hooks and keyboard input dies. Just check vk + flag, queue, return.
- **nCode < 0 → pass through immediately** (`CallNextHookEx`) per MSDN.
- **Tkinter is single-threaded** — never touch Tk vars/after() from hooks/threads; use a plain
  flag + `queue.Queue` polled via `root.after`.
- **ctypes**: `GetLastError` lives in kernel32, not user32; hook thread needs its own
  GetMessage loop; always wrap thread bodies in try/except → `log()` (pythonw hides crashes).
- **Overlays**: single fullscreen `-transparentcolor` Toplevel beats many child windows
  (sync/hit-testing solved by construction); transparent pixels pass mouse input through.
- **pythonw debugging**: all diagnostics via `scrollshot.log`, never stderr.
