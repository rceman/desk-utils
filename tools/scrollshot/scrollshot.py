"""ScrollShot: pick a zone (green border), press Start, scroll, get one long PNG."""
import argparse, ctypes, datetime, os, queue, threading, time
import numpy as np
from PIL import Image, ImageDraw, ImageTk

# Must run before any window is created: logical px == physical px for mss/grab
try:
    ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # PER_MONITOR_AWARE_V2
except Exception:
    try:
        ctypes.windll.shcore.SetProcessDpiAwareness(2)
    except Exception:
        try:
            ctypes.windll.user32.SetProcessDPIAware()
        except Exception:
            pass

FPS = 20
MAX_DY = 260
MIN_DY = 6
THRESH = 12.0
IDLE_SEC = 10.0
LOG_PATH = None


def log(msg):
    try:
        if LOG_PATH:
            with open(LOG_PATH, "a", encoding="utf-8") as f:
                f.write(f"{datetime.datetime.now():%H:%M:%S.%f}"[:-3] + f" {msg}\n")
    except Exception:
        pass


def grab(zone):
    import mss
    x, y, w, h = zone
    with mss.mss() as sct:
        shot = sct.grab({"left": x, "top": y, "width": w, "height": h})
    return np.array(Image.frombytes("RGB", shot.size, shot.bgra, "raw", "BGRX"))


def find_dy(prev, new):
    """Signed vertical shift: >0 down, <0 up, 0 = no motion."""
    ga = prev.astype(np.float32)
    gb = new.astype(np.float32)
    h = ga.shape[0]
    if h < 100:
        return 0, 999.0
    pa, pb = ga.mean(axis=1), gb.mean(axis=1)
    if float(pa.std()) < 1.5 or float(pb.std()) < 1.5:
        return 0, 999.0
    base = float(np.abs(ga - gb).mean())   # zero-motion pixel error
    lim = min(max(MAX_DY, h - 100), h - 60)
    cands = []
    for dy in list(range(1, lim + 1)) + list(range(-1, -lim - 1, -1)):
        if dy > 0:
            a, b = pa[dy:], pb[:h - dy]
        else:
            a, b = pa[:h + dy], pb[-dy:]
        n = min(len(a), len(b))
        if n < 20:
            continue
        cands.append((float(np.abs(a[:n] - b[:n]).mean()), dy))
    if not cands:
        return 0, base
    cands.sort()
    best = cands[0][0]
    winners = []
    for _, dy in [c for c in cands if c[0] <= best * 1.5 + 0.5][:12]:
        if dy > 0:
            a, b = ga[dy:], gb[:h - dy]
        else:
            a, b = ga[:h + dy], gb[-dy:]
        v = float(np.abs(a - b).mean())
        if v < base * 0.6 and v < THRESH * 2:
            winners.append((v, abs(dy), dy))
    if winners:
        winners.sort()
        return winners[0][2], winners[0][0]
    return 0, base


def select_zone(root, min_h=100):
    import tkinter as tk
    root = tk.Toplevel(root)
    root.attributes("-fullscreen", True)
    root.attributes("-alpha", 0.25)
    root.attributes("-topmost", True)
    root.configure(bg="black")
    cv = tk.Canvas(root, cursor="cross", bg="black", highlightthickness=0)
    cv.pack(fill="both", expand=True)
    st = {"x0": 0, "y0": 0, "rect": None}
    cv.create_text(20, 20, anchor="nw", fill="#00ff88", font=("Consolas", 16),
                   text="Drag to select zone   ·   Esc = cancel")
    cv.bind("<ButtonPress-1>", lambda e: (st.update(x0=e.x_root, y0=e.y_root),
                                          cv.delete(st["rect"]) if st["rect"] else None,
                                          st.update(rect=cv.create_rectangle(e.x_root, e.y_root, e.x_root, e.y_root,
                                                                             outline="#00ff88", width=2))))
    cv.bind("<B1-Motion>", lambda e: st["rect"] and cv.coords(st["rect"], st["x0"], st["y0"], e.x_root, e.y_root))
    def rel(e):
        x, y = min(st["x0"], e.x_root), min(st["y0"], e.y_root)
        w, h = abs(e.x_root - st["x0"]), abs(e.y_root - st["y0"])
        if w > 30 and h >= min_h:
            st["zone"] = (x, y, w, h)
        root.destroy()
    cv.bind("<ButtonRelease-1>", rel)
    root.bind("<Escape>", lambda e: root.destroy())
    root.wait_window()
    return st.get("zone")


def intersects(px, py, pw, ph, zone):
    x, y, w, h = zone
    return not (px + pw <= x or px >= x + w or py + ph <= y or py >= y + h)


def set_dark_title(root, dark):
    try:
        from ctypes import wintypes
        user32, dwm = ctypes.windll.user32, ctypes.windll.dwmapi
        user32.GetParent.argtypes = [wintypes.HWND]
        user32.GetParent.restype = wintypes.HWND
        dwm.DwmSetWindowAttribute.argtypes = [wintypes.HWND, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD]
        dwm.DwmSetWindowAttribute.restype = ctypes.c_long
        hwnd = user32.GetParent(root.winfo_id()) or root.winfo_id()
        v = ctypes.c_int(1 if dark else 0)
        hr = dwm.DwmSetWindowAttribute(hwnd, 20, ctypes.byref(v), ctypes.sizeof(v))
        log(f"dark title {dark} hwnd={hwnd} hr={hr}")
    except Exception as e:
        log(f"dark title error: {e}")


class Border:
    """Windows overlay; zone and handle dimensions are physical pixels."""

    T = 6
    C = 14
    G = 24
    GAP = 4
    MIN_W = 30
    MIN_H = 100
    GREEN = "#00ff88"
    INK = "#0d3320"
    KEY = "#010203"
    MODES = ("n", "s", "w", "e", "nw", "ne", "sw", "se", "move")
    HIT_ORDER = ("nw", "ne", "sw", "se", "move", "n", "s", "w", "e")

    def __init__(self, tk, zone, on_change=None):
        x, y, w, h = map(int, zone)
        if w < self.MIN_W or h < self.MIN_H:
            raise ValueError("Border needs a zone at least 30 x 100 pixels")
        self.tk = tk
        self.zone = [x, y, w, h]
        self.on_change = on_change or (lambda z: None)
        self._closed = False
        self._drag = None
        self._cursor = None

        self.win = tk.Toplevel()
        self.win.withdraw()  # Configure and draw before showing any pixels.
        if self.win.tk.call("tk", "windowingsystem") != "win32":
            self.win.destroy()
            raise RuntimeError("This Border uses Windows color-key transparency")
        self.win.overrideredirect(True)
        self.win.configure(bg=self.KEY, bd=0, highlightthickness=0)
        self.win.attributes("-topmost", True)
        self.win.attributes("-toolwindow", True)
        self.win.attributes("-transparentcolor", self.KEY)
        # Keep the window opaque; only KEY pixels are transparent.
        # Do not set WS_EX_TRANSPARENT: it would disable all handle input.

        self._vx = self.win.winfo_vrootx()
        self._vy = self.win.winfo_vrooty()
        vw = self.win.winfo_vrootwidth()
        vh = self.win.winfo_vrootheight()
        self.win.maxsize(vw, vh)
        # Tk/Windows accepts '+-1920' as an absolute negative coordinate.
        # Formatting a negative coordinate as '-1920' anchors from the right.
        self.win.geometry(f"{vw}x{vh}+{self._vx}+{self._vy}")
        self.canvas = tk.Canvas(
            self.win, bg=self.KEY, bd=0, highlightthickness=0,
            relief="flat", takefocus=0,
        )
        self.canvas.pack(fill="both", expand=True)
        self._cursors = {
            "n": "sb_v_double_arrow", "s": "sb_v_double_arrow",
            "w": "sb_h_double_arrow", "e": "sb_h_double_arrow",
            "nw": "size_nw_se", "se": "size_nw_se",
            "ne": "size_ne_sw", "sw": "size_ne_sw", "move": "fleur",
        }
        self._items = {}
        for mode in self.MODES:
            tags = ("handle", "corner") if mode in self.HIT_ORDER[:4] else ("handle",)
            self._items[mode] = self.canvas.create_rectangle(
                0, 0, 0, 0, fill=self.GREEN, outline="", width=0, tags=tags,
            )
        self._dots = [
            self.canvas.create_rectangle(
                0, 0, 0, 0, fill=self.INK, outline="", width=0, tags=("dots",),
            ) for _ in range(6)
        ]
        # Bind exactly once, on the canvas, not on both child and Toplevel.
        self.canvas.bind("<ButtonPress-1>", self._press)
        self.canvas.bind("<B1-Motion>", self._motion)
        self.canvas.bind("<ButtonRelease-1>", self._release)
        self.canvas.bind("<Motion>", self._hover)
        self.canvas.bind("<Leave>", self._leave)
        self.layout()
        self.win.deiconify()
        self.win.update_idletasks()
        self._suppress_native_frame()

    def _suppress_native_frame(self):
        """Apply frame policy to Tk's native wrapper, not its child HWND."""
        import ctypes
        from ctypes import wintypes

        user32, dwm = ctypes.windll.user32, ctypes.windll.dwmapi
        user32.GetAncestor.argtypes = [wintypes.HWND, wintypes.UINT]
        user32.GetAncestor.restype = wintypes.HWND
        dwm.DwmSetWindowAttribute.argtypes = [
            wintypes.HWND, wintypes.DWORD, ctypes.c_void_p, wintypes.DWORD,
        ]
        dwm.DwmSetWindowAttribute.restype = ctypes.c_long
        hwnd = user32.GetAncestor(self.win.winfo_id(), 2)  # GA_ROOT
        self._frame_results = []
        # Non-client rendering off; square corners; no Windows 11 border.
        # Attributes 33/34 may return E_INVALIDARG on older Windows versions.
        for attr, value in ((2, 1), (33, 1), (34, 0xFFFFFFFE)):
            data = wintypes.DWORD(value)
            hr = dwm.DwmSetWindowAttribute(
                hwnd, attr, ctypes.byref(data), ctypes.sizeof(data),
            )
            self._frame_results.append((attr, hr))

    @classmethod
    def _rectangles(cls, zone):
        """Absolute, half-open rectangles; none intersects the capture zone."""
        x, y, w, h = zone
        r, b = x + w, y + h
        t, c, g = cls.T, cls.C, cls.G
        gx = x + (w - g) // 2
        gy = y - t - cls.GAP - g
        return {
            "n": (x - t, y - t, r + t, y),
            "s": (x - t, b, r + t, b + t),
            "w": (x - t, y, x, b),
            "e": (r, y, r + t, b),
            "nw": (x - c, y - c, x, y),
            "ne": (r, y - c, r + c, y),
            "sw": (x - c, b, x, b + c),
            "se": (r, b, r + c, b + c),
            "move": (gx, gy, gx + g, gy + g),
        }

    def _coords(self, item, rect):
        l, t, r, b = rect
        self.canvas.coords(item, l - self._vx, t - self._vy,
                           r - self._vx, b - self._vy)

    def layout(self):
        if self._closed:
            return
        self._rects = self._rectangles(self.zone)
        # Every handle is updated from the SAME zone on EVERY layout pass.
        for mode, rect in self._rects.items():
            self._coords(self._items[mode], rect)
        gx, gy, _, _ = self._rects["move"]
        # Six crisp 2px squares; no font, raster scaling, padding or alpha.
        for item, (col, row) in zip(
            self._dots, ((c, r) for r in range(3) for c in range(2))
        ):
            px, py = gx + 8 + 6 * col, gy + 5 + 6 * row
            self._coords(item, (px, py, px + 2, py + 2))
        self.canvas.tag_raise("corner")
        self.canvas.tag_raise(self._items["move"])
        self.canvas.tag_raise("dots")

    def _hit(self, rx, ry):
        # Corner precedence is explicit, independent of Canvas item order.
        for mode in self.HIT_ORDER:
            l, t, r, b = self._rects[mode]
            if l <= rx < r and t <= ry < b:
                return mode
        return None

    def _set_cursor(self, mode):
        cursor = self._cursors.get(mode, "")
        if cursor != self._cursor:
            self.canvas.configure(cursor=cursor)
            self._cursor = cursor

    def _hover(self, e):
        if not self._closed and self._drag is None:
            self._set_cursor(self._hit(e.x_root, e.y_root))

    def _leave(self, e):
        if not self._closed and self._drag is None:
            self._set_cursor(None)

    def _press(self, e):
        if self._closed:
            return "break"
        mode = self._hit(e.x_root, e.y_root)
        if mode is not None:
            self._drag = (mode, e.x_root, e.y_root, tuple(self.zone))
            self._set_cursor(mode)
            self.canvas.grab_set()
        return "break"

    @classmethod
    def _dragged_zone(cls, start, mode, dx, dy):
        x, y, w, h = start
        if mode == "move":
            return [x + dx, y + dy, w, h]
        l, t, r, b = x, y, x + w, y + h
        if "w" in mode:
            l = min(x + dx, r - cls.MIN_W)
        if "e" in mode:
            r = max(x + w + dx, l + cls.MIN_W)
        if "n" in mode:
            t = min(y + dy, b - cls.MIN_H)
        if "s" in mode:
            b = max(y + h + dy, t + cls.MIN_H)
        return [l, t, r - l, b - t]

    def _motion(self, e):
        if self._closed or self._drag is None:
            return "break"
        mode, px, py, start = self._drag
        zone = self._dragged_zone(start, mode, e.x_root - px, e.y_root - py)
        if zone != self.zone:
            self.zone = zone
            self.layout()
            self.on_change(tuple(zone))
        return "break"

    def _ungrab(self):
        try:
            if self.canvas.grab_current() is self.canvas:
                self.canvas.grab_release()
        except self.tk.TclError:
            pass  # The owner may already have destroyed the Tk hierarchy.

    def _release(self, e):
        if self._closed:
            return "break"
        try:
            self._motion(e)  # Include the final pointer position.
        finally:
            self._drag = None
            self._ungrab()
        if not self._closed:
            self._set_cursor(self._hit(e.x_root, e.y_root))
        return "break"

    def close(self):
        if self._closed:
            return
        self._closed = True
        self._drag = None
        self._ungrab()
        try:
            self.win.destroy()
        except self.tk.TclError:
            pass


class Panel:
    def __init__(self, tk, out_dir):
        self.tk = tk
        self.out_dir = out_dir
        self.out_path = None
        self.zone = None
        self.border = None
        self.recording = False
        self.stop_flag = threading.Event()
        self.cancel_flag = threading.Event()
        self.start_flag = threading.Event()
        self.q = queue.Queue()
        self.root = tk.Tk()
        self.root.title("ScrollShot")
        self.root.attributes("-topmost", True)
        self.root.configure(bg="#1e1e1e")
        self.root.resizable(False, False)
        self.status = tk.StringVar(value="Select Area to begin")
        self._st = "Select Area to begin"
        self._inf = ""
        tk.Label(self.root, textvariable=self.status, bg="#1e1e1e", fg="#d0d0d0",
                 font=("Consolas", 9), justify="center", wraplength=360, height=1,
                 anchor="center").pack(padx=10, pady=(6, 0), fill="x")
        self.view = tk.Canvas(self.root, bg="#111111", width=360, height=200, highlightthickness=0)
        self.view.pack(padx=10, pady=(2, 0))
        self.rec = tk.StringVar(value="")
        tk.Label(self.root, textvariable=self.rec, bg="#1e1e1e", fg="#c08f4f",
                 font=("Consolas", 8), height=0, anchor="e", justify="right").pack(padx=10, fill="x")
        global LOG_PATH
        self.folder = out_dir
        os.makedirs(self.folder, exist_ok=True)
        LOG_PATH = os.path.join(self.folder, "scrollshot.log")

        self.scroll_iv = 0.25
        self.auto_scroll = tk.BooleanVar(value=False)
        self.prtscr = tk.BooleanVar(value=True)
        self._prtscr_flag = True
        self.copy_as_path = tk.BooleanVar(value=True)
        row = tk.Frame(self.root, bg="#1e1e1e")
        row.pack(anchor="w", padx=10)
        tk.Checkbutton(row, text="Auto scroll", variable=self.auto_scroll, bg="#1e1e1e",
                       fg="#d0d0d0", selectcolor="#333333", activebackground="#1e1e1e",
                       activeforeground="#ffffff", font=("Consolas", 9)).pack(side="left")
        self._ivbtn = tk.Button(row, text="1.0s", command=self.cycle_iv, bg="#3d3d3d", fg="#a0d0a0",
                                font=("Consolas", 8), width=5)
        self._ivbtn.pack(side="left", padx=4)
        self.border_var = tk.BooleanVar(value=True)
        tk.Checkbutton(row, text="Show Area border", variable=self.border_var,
                       command=self.apply_border, bg="#1e1e1e", fg="#d0d0d0",
                       selectcolor="#333333", activebackground="#1e1e1e",
                       activeforeground="#ffffff",
                       font=("Consolas", 9)).pack(side="left", padx=8)
        btns = tk.Frame(self.root, bg="#1e1e1e")
        btns.pack(pady=(4, 6), fill="x")
        r0 = tk.Frame(btns, bg="#1e1e1e"); r0.pack()
        self.btn_area = tk.Button(r0, text="Select Area", command=self.area_click, bg="#4a2d6a",
                                  fg="white", font=("Consolas", 10), width=18)
        self.btn_area.pack(side="left", padx=4, pady=2)
        self.btn_rec = tk.Button(r0, text="Start", command=self.toggle_rec, bg="#2d6a2d",
                                 fg="white", disabledforeground="#909090",
                                 font=("Consolas", 10), width=18, state="disabled")
        self.btn_rec.pack(side="left", padx=4, pady=2)
        r1 = tk.Frame(btns, bg="#1e1e1e"); r1.pack()
        tk.Button(r1, text="Take screenshot", command=self.shot, bg="#2d5a6a", fg="white",
                  font=("Consolas", 10), width=18).pack(side="left", padx=4, pady=2)
        tk.Button(r1, text="Open folder", command=self.open_folder, bg="#2d4a6a", fg="white",
                  font=("Consolas", 10), width=18).pack(side="left", padx=4, pady=2)


        r3 = tk.Frame(btns, bg="#1e1e1e"); r3.pack()
        tk.Button(r3, text="Copy image", command=self.copy_image, bg="#3d3d3d", fg="white",
                  font=("Consolas", 10), width=18).pack(side="left", padx=4, pady=2)
        tk.Button(r3, text="Copy image path", command=self.copy_path, bg="#3d3d3d", fg="white",
                  font=("Consolas", 10), width=18).pack(side="left", padx=4, pady=2)
        r4 = tk.Frame(self.root, bg="#1e1e1e")
        r4.pack(anchor="w", padx=10, pady=(2, 0))
        for text, var in [("PrtScr = Take screenshot", self.prtscr),
                          ("screenshot → path", self.copy_as_path)]:
            tk.Checkbutton(r4, text=text, variable=var, bg="#1e1e1e", fg="#d0d0d0",
                           selectcolor="#333333", activebackground="#1e1e1e",
                           activeforeground="#ffffff", font=("Consolas", 8),
                           command=self._sync_flags).pack(side="left", padx=4)
        foot = tk.Frame(self.root, bg="#1e1e1e")
        foot.pack(fill="x", padx=10, pady=(4, 14))
        short = self.folder.replace(os.path.expanduser("~"), "~")
        tk.Label(foot, text=f"Folder: {short}", bg="#1e1e1e", fg="#7aa2c8",
                 font=("Consolas", 8), anchor="w").pack(side="left", fill="x", expand=True)
        tk.Button(foot, text="↻", command=self.reload, bg="#3d3d3d", fg="#a0d0a0",
                  font=("Consolas", 9), width=2, relief="flat").pack(side="right")
        def _cbexc(exc, val, tb):
            import traceback
            log("callback error:\n" + "".join(traceback.format_exception(exc, val, tb)))
            self._st = "error — see log"; self._render()
        self.root.report_callback_exception = _cbexc
        self.root.update_idletasks()
        sw, sh = self.root.winfo_screenwidth(), self.root.winfo_screenheight()
        pw, ph = self.root.winfo_reqwidth(), self.root.winfo_reqheight()
        self.root.geometry(f"+{max(0, sw - pw - 10)}+{max(0, sh - ph - 150)}")
        self.root.bind("<Escape>", lambda e: self.toggle_rec() if self.recording else None)
        self.root.protocol("WM_DELETE_WINDOW", self.cancel)
        self.photo = None
        self.worker = threading.Thread(target=self.record_loop, daemon=True)
        log("panel ready")

    def put(self, kind, payload):
        self.q.put((kind, payload))

    def area_click(self):
        if self.recording:
            self._st = "Finish recording first"; self._render()
            return
        if self.zone:
            self.zone = None
            self.root.title("ScrollShot")
            self.btn_rec.config(state="disabled")
            self.btn_area.config(text="Select Area")
            self.apply_border()
            self._st = "Select Area to begin"; self._render()
            log("zone cleared")
        else:
            self.select_area()

    def _zone_changed(self, z):
        self.zone = z
        self.root.title(f"ScrollShot  {z[2]}x{z[3]}")

    def select_area(self):
        try:
            self._select_area()
        except Exception:
            import traceback
            log(f"select_area error:\n{traceback.format_exc()}")
            self._st = "select failed — see log"; self._render()

    def _select_area(self):
        if self.recording:
            self._st = "Finish recording first"; self._render()
            return
        zone = select_zone(self.root)
        if zone is None:
            # cancelled -> clear current zone, Start disabled again
            self.zone = None
            self.root.title("ScrollShot")
            self.btn_rec.config(state="disabled")
            self.btn_area.config(text="Select Area")
            self.apply_border()
            self._st = "Select Area to begin"; self._render()
            log("zone cleared")
            return
        self.zone = zone
        self.root.title(f"ScrollShot  {zone[2]}x{zone[3]}")
        self.btn_rec.config(state="normal")
        self.btn_area.config(text="Cancel Area select")
        self.apply_border()
        self._st = "Press Start"; self._render()
        log(f"zone={zone}")

    def apply_border(self):
        if self.border:
            self.border.close()
            self.border = None
        if self.border_var.get() and self.zone:
            self.border = Border(self.tk, self.zone, self._zone_changed)

    def _place(self, zone):
        x, y, w, h = zone
        sw, sh = self.root.winfo_screenwidth(), self.root.winfo_screenheight()
        pw, ph = 400, 540
        for cx, cy in [(x + w + 12, y), (x - pw - 12, y), (x, y + h + 12), (x, y - ph - 12),
                       (sw - pw - 12, 12), (12, 12), (sw - pw - 12, sh - ph - 12), (12, sh - ph - 12)]:
            cx, cy = max(0, min(cx, sw - pw)), max(0, min(cy, sh - ph))
            if not intersects(cx, cy, pw, ph, zone):
                return f"{pw}x{ph}+{cx}+{cy}"
        log("no safe placement — panel may overlap zone")
        return f"{pw}x{ph}+{max(0, sw - pw - 12)}+12"

    def _sys_dark(self):
        try:
            import winreg
            v = winreg.QueryValueEx(winreg.OpenKey(winreg.HKEY_CURRENT_USER,
                r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
                "AppsUseLightTheme")[0]
            return v == 0
        except Exception:
            return True

    def start(self):
        self.root.update_idletasks()
        self.root.after(150, lambda: set_dark_title(self.root, self._sys_dark()))
        self.worker.start()
        threading.Thread(target=self._prtscr_hook, daemon=True).start()
        self.root.after(80, self.poll)
        self.root.mainloop()

    def cycle_iv(self):
        ivs = [1.0, 0.5, 0.25, 2.0]
        self.scroll_iv = ivs[(ivs.index(self.scroll_iv) + 1) % len(ivs)]
        self._ivbtn.config(text=f"{self.scroll_iv}s")
        log(f"scroll interval {self.scroll_iv}")

    def toggle_rec(self):
        if not self.recording:
            if not self.zone:
                return
            self.root.update_idletasks()
            if intersects(self.root.winfo_x(), self.root.winfo_y(),
                          self.root.winfo_width(), self.root.winfo_height(), self.zone):
                self._st = "Panel overlaps zone — move it or reselect"; self._render()
                log("start refused: panel overlaps zone")
                return
            self.stop_flag.clear()
            self.start_flag.set()
            self.recording = True
            self.btn_rec.config(text="Stop & Save", bg="#4a4a2d")
            self._st = "Recording"; self._render()
        else:
            self.stop_flag.set()
            self.btn_rec.config(state="disabled")
            self._st = "Stopping…"; self._render()

    def shot(self):
        if self.recording:
            self._st = "Finish recording first"; self._render()
            return
        zone = select_zone(self.root, min_h=20)
        if zone is None:
            return
        try:
            img = grab(zone)
            dest = self._new_path()
            Image.fromarray(img).save(dest)
            self.out_path = dest
            self.put("preview", img)
            self._st = "Screenshot saved"; self._render()
            self._inf = f"{img.shape[1]}x{img.shape[0]}"; self._render()
            log(f"shot saved {self.out_path}")
            if self.copy_as_path.get():
                self.copy_path()
            else:
                self.copy_image()
        except Exception as e:
            log(f"shot error: {e}")
            self._st = f"shot error: {e}"; self._render()

    def _new_path(self):
        import uuid
        base = f"scrollshot_{datetime.datetime.now():%Y%m%d_%H%M%S}_{uuid.uuid4().hex[:6]}"
        return os.path.join(self.folder, f"{base}.png")

    def poll(self):
        try:
            while True:
                kind, payload = self.q.get_nowait()
                if kind == "status":
                    self._st = payload
                    self._render()
                elif kind == "info":
                    self._inf = payload
                    self._render()
                elif kind == "rec":
                    self.rec.set(payload)
                elif kind == "shot":
                    self.shot()
                elif kind == "preview":
                    self.show(payload)
                elif kind == "btn":
                    self.recording = False
                    self.btn_rec.config(text="Start", bg="#2d6a2d", state="normal" if self.zone else "disabled")
        except queue.Empty:
            pass
        self.root.after(80, self.poll)

    def _render(self):
        self.status.set(f"{self._st}   {self._inf}" if self._inf else self._st)

    def show(self, arr):
        try:
            img = Image.fromarray(arr)
            w, h = img.size
            if h > 1200:
                img = img.crop((0, h - 1200, w, h))
                w, h = img.size
            s = min(360 / w, 200 / h, 1.0)
            if s < 1.0:
                img = img.resize((max(1, int(w * s)), max(1, int(h * s))), Image.BILINEAR)
            if img.height > 200:
                img = img.crop((0, img.height - 200, img.width, img.height))
            self.photo = ImageTk.PhotoImage(img)
            self.view.delete("all")
            self.view.create_image(180, 100, image=self.photo)
        except Exception as e:
            log(f"show error: {e}")

    def open_folder(self):
        try:
            import subprocess
            if self.out_path and os.path.exists(self.out_path):
                subprocess.Popen(["explorer.exe", f"/select,{self.out_path}"])
            else:
                os.startfile(self.folder)
        except Exception as e:
            log(f"open folder: {e}")

    def copy_image(self):
        try:
            if not (self.out_path and os.path.exists(self.out_path)):
                self._st = "Nothing to copy yet"; self._render()
                return
            import subprocess
            q = self.out_path.replace("'", "''")
            ps = ('Add-Type -AssemblyName System.Drawing,System.Windows.Forms;'
                  f"$img=[System.Drawing.Image]::FromFile('{q}');"
                  '[System.Windows.Forms.Clipboard]::SetImage($img);$img.Dispose()')
            subprocess.Popen(["powershell", "-NoProfile", "-WindowStyle", "Hidden", "-Command", ps],
                             creationflags=0x08000000)
            self._st = "Image copied to clipboard"; self._render()
            log(f"copy {self.out_path}")
        except Exception as e:
            log(f"copy error: {e}")

    def copy_path(self):
        try:
            path = self.out_path if (self.out_path and os.path.exists(self.out_path)) else self.folder
            self.root.clipboard_clear()
            self.root.clipboard_append(path)
            self._st = "Path copied to clipboard"; self._render()
        except Exception as e:
            log(f"copy path: {e}")

    def reload(self):
        import sys, subprocess
        log("reload")
        subprocess.Popen([sys.executable, os.path.abspath(__file__)])
        self.root.after(200, self.root.destroy)

    def _sync_flags(self):
        self._prtscr_flag = self.prtscr.get()

    def _prtscr_hook(self):
        import ctypes
        from ctypes import wintypes
        try:
            self._hook_body()
        except Exception as e:
            log(f"kb hook crashed: {e!r}")

    def _hook_body(self):
        import ctypes
        from ctypes import wintypes
        u = ctypes.windll.user32
        ok = u.RegisterHotKey(None, 1, 0, 0x2C)  # VK_SNAPSHOT
        log(f"hotkey={ok} err={ctypes.windll.kernel32.GetLastError()}")
        m = wintypes.MSG()
        while u.GetMessageW(ctypes.byref(m), None, 0, 0) > 0:
            if m.message == 0x0312 and self._prtscr_flag:
                self.put("shot", None)

    def cancel(self):
        self.cancel_flag.set()
        self.stop_flag.set()
        if self.border:
            self.border.close()
        self.root.destroy()

    def record_loop(self):
        while not self.cancel_flag.is_set():
            while not self.start_flag.is_set():
                if self.cancel_flag.is_set():
                    return
                time.sleep(0.05)
            zone = self.zone
            chunks, top_chunks = [], []
            canvas = None
            anchor = None
            pos = top = cap_h = 0
            frames = scrolls = 0
            self.put("status", "Recording")
            last_new = time.monotonic()
            last_tick = 0.0
            try:
                while not self.stop_flag.is_set():
                    if self.auto_scroll.get() and time.monotonic() - last_tick >= self.scroll_iv:
                        last_tick = time.monotonic()
                        try:
                            import ctypes
                            u = ctypes.windll.user32
                            u.SetCursorPos(zone[0] + zone[2] // 2, zone[1] + zone[3] // 2)
                            u.mouse_event(0x0800, 0, 0, -120, 0)
                        except Exception as e:
                            log(f"autoscroll: {e}")
                    t0 = time.monotonic()
                    f = grab(zone)
                    frames += 1
                    fh = f.shape[0]
                    if canvas is None:
                        canvas, anchor, cap_h = f, f, fh
                        self.put("preview", canvas)
                    else:
                        if frames % 5 == 0:
                            self.put("preview", f)
                        dy, score = find_dy(anchor, f)
                        if dy:
                            anchor = f
                            scrolls += 1
                            last_new = time.monotonic()
                            new_pos = pos + dy
                            if new_pos < top:
                                n = min(top - new_pos, fh)
                                top_chunks.insert(0, f[:n].copy())
                                top, cap_h = new_pos, cap_h + n
                            elif new_pos + fh > top + cap_h:
                                st = top + cap_h - new_pos
                                chunks.append(f[st:].copy())
                                cap_h += fh - st
                            pos = new_pos
                            self.put("info", f"{frames}f · {scrolls}s · {cap_h}px")
                    left = IDLE_SEC - (time.monotonic() - last_new)
                    self.put("rec", f"auto-save in {max(0, int(left + 0.999))}s")
                    if left <= 0:
                        log("idle stop")
                        break
                    dt = 1.0 / FPS - (time.monotonic() - t0)
                    if dt > 0:
                        time.sleep(dt)
            except Exception as e:
                import traceback
                log(f"record error: {e}\n{traceback.format_exc()}")
                self.put("status", f"error: {e}")
                self.put("info", "failed — nothing saved")
                self.start_flag.clear()
                self.stop_flag.clear()
                self.put("rec", "")
                self.put("btn", "start")
                continue
            if self.cancel_flag.is_set():
                log("cancelled")
                return
            if canvas is not None:
                ok = True
                if chunks or top_chunks:
                    try:
                        canvas = np.vstack([*top_chunks, canvas, *chunks])
                    except Exception as e:
                        log(f"concat error: {e}")
                        self.put("status", f"concat error — session lost")
                        ok = False
                if self.cancel_flag.is_set():
                    return
                if ok:
                    try:
                        dest = self._new_path()
                        tmp = dest + ".tmp.png"
                        Image.fromarray(canvas).save(tmp)
                        if self.cancel_flag.is_set():
                            os.remove(tmp)
                            return
                        os.replace(tmp, dest)
                        self.out_path = dest
                        log(f"saved {dest} {canvas.shape}")
                        self.put("preview", canvas)
                        self.put("status", "Saved — Start again")
                        self.put("info", f"{frames}f · {scrolls}s · {canvas.shape[0]}px")
                    except Exception as e:
                        log(f"save error: {e}")
                        self.put("status", f"save error: {e}")
            self.start_flag.clear()
            self.stop_flag.clear()
            self.put("btn", "start")


def selftest():
    rng = np.random.default_rng(0)
    tall = rng.random((4000, 400, 3)) * 255
    tall = tall.astype(np.uint8)
    tall[::30] = 30
    h = 400
    for step in (8, 25, 90, 200):
        pos, frames = 0, [tall[:h].copy()]
        for i in range(14):
            pos += step
            frames.append(tall[pos:pos + h].copy())
        canvas, anchor, top, cap_h, pos_d = frames[0], frames[0], 0, h, 0
        for f in frames[1:]:
            dy, sc = find_dy(anchor, f)
            anchor = f
            new_pos = pos_d + dy
            if new_pos < top:
                canvas = np.vstack([f[:min(top - new_pos, h)], canvas])
                top, cap_h = new_pos, cap_h + min(top - new_pos, h)
            elif new_pos + h > top + cap_h:
                st = top + cap_h - new_pos
                canvas = np.vstack([canvas, f[st:]])
                cap_h += h - st
            pos_d = new_pos
        expected = h + 14 * step
        assert canvas.shape[0] == expected, f"step {step}: canvas {canvas.shape[0]} != {expected}"
        ref = tall[:canvas.shape[0]]
        diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
        print(f"step={step:3d} canvas={canvas.shape[0]} diff={diff:.3f}")
        assert diff == 0, f"step {step} mismatch"
    print("selftest OK")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    if args.selftest:
        selftest()
        return
    import tkinter as tk
    out_dir = (os.path.dirname(os.path.abspath(args.out)) if args.out else
               os.path.join(os.path.expanduser("~"), "Pictures", "Screenshots", "ScrollShot"))
    os.makedirs(out_dir, exist_ok=True)
    global LOG_PATH
    LOG_PATH = os.path.join(out_dir, "scrollshot.log")
    log("start")
    try:
        Panel(tk, out_dir).start()
    except Exception:
        import traceback
        log(f"fatal: {traceback.format_exc()}")


if __name__ == "__main__":
    main()
