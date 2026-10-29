# Agent references

- **Rust idioms / review:** BeautifulRust skills cloned at `~/git/BeautifulRust/skills/`
  (`rust-best-practices`, `rust-port`, `rust-review`; origin github.com/morganlinton/BeautifulRust,
  www.beautifulrust.dev). Consult before writing/reviewing Rust.
- **Sol review workflow:** `tools/scrollshot/SOL_REVIEW.md` — airelay review_sol session usage.
- **ScrollShot deploy:** `python tools/scrollshot/activate.py --restart` (copies to
  `C:\Tools\diskhound\tools\scrollshot\`, restarts pythonw).
- **ScrollShot runtime:** log at `C:\Users\therceman\Pictures\Screenshots\ScrollShot\scrollshot.log`.
  Verify UI changes via log (`start`, `panel ready`, `kb hook=`) — silent Tk callback/hook
  crashes are invisible under pythonw. Wrap hook/worker bodies in try/except → `log()`.
