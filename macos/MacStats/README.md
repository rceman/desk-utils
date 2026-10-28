# MacStats

Native macOS menu bar widget (Swift/AppKit, no dependencies). Two rows per column: label on top, value below.

Columns: CPU, RAM, DSK, TMP (CPU temperature), DVN (Devin weekly quota left), CDX (Codex 5-hour limit left).

Settings in the dropdown menu: column spacing, font size, vertical offset, and which columns to show.

Build: `./build.sh` installs `~/Applications/MacStats.app`.

Notes:
- CPU temperature uses private IOKit HID APIs (Apple Silicon, no root).
- DVN reads `~/.local/share/devin/credentials.toml` and CDX reads `~/.codex/auth.json`; both call undocumented endpoints and show `n/a` if they stop working.
