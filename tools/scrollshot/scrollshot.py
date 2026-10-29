"""ScrollShot: pick a zone (green border), press Start, scroll, get one long PNG."""
import argparse, ctypes, datetime, os, queue, threading, time
import numpy as np
from PIL import Image, ImageTk

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
    T = 6

    def __init__(self, tk, zone, on_change=None):
        self.zone = list(zone)
        self.on_change = on_change or (lambda z: None)
        self.wins = []
        for edge, cur in [("top", "sb_v_double_arrow"), ("bottom", "sb_v_double_arrow"),
                          ("left", "sb_h_double_arrow"), ("right", "sb_h_double_arrow")]:
            win = tk.Toplevel()
            win.overrideredirect(True)
            win.attributes("-topmost", True)
            win.configure(bg="#00ff88", cursor=cur)
            win.bind("<B1-Motion>", lambda e, ed=edge: self._move(ed, e.x_root, e.y_root))
            self.wins.append(win)
        self.grip = tk.Toplevel()
        self.grip.overrideredirect(True)
        self.grip.attributes("-topmost", True)
        self.grip.configure(cursor="fleur")
        cv = tk.Canvas(self.grip, width=22, height=22, bg="#ffcc00", highlightthickness=0,
                       cursor="fleur")
        cv.pack()
        cv.bind("<ButtonPress-1>", self._gstart)
        cv.bind("<B1-Motion>", self._gmove)
        c = "#333333"
        for x0, y0, x1, y1 in [(11, 5, 11, 2), (11, 17, 11, 20), (5, 11, 2, 11), (17, 11, 20, 11)]:
            cv.create_line(x0, y0, x1, y1, fill=c, width=2, arrow="last", arrowshape=(4, 5, 2))
        cv.create_oval(9, 9, 13, 13, fill=c, outline="")
        self.grip.bind("<ButtonPress-1>", self._gstart)
        self.grip.bind("<B1-Motion>", self._gmove)
        self._g = None
        self.layout()

    def layout(self):
        x, y, w, h = self.zone
        t = self.T
        geoms = [(x - t, y - t, w + 2 * t, t), (x - t, y + h, w + 2 * t, t),
                 (x - t, y, t, h), (x + w, y, t, h)]
        for win, (bx, by, bw, bh) in zip(self.wins, geoms):
            win.geometry(f"{bw}x{bh}+{bx}+{by}")
        self.grip.geometry(f"22x22+{x + w + t - 5}+{y - t - 17}")

    def _move(self, edge, rx, ry):
        x, y, w, h = self.zone
        if edge == "top":
            ny = min(ry, y + h - 100)
            h += y - ny
            y = ny
        elif edge == "bottom":
            h = max(100, ry - y)
        elif edge == "left":
            nx = min(rx, x + w - 30)
            w += x - nx
            x = nx
        elif edge == "right":
            w = max(30, rx - x)
        self.zone = [x, y, w, h]
        self.layout()
        self.on_change(tuple(self.zone))

    def _gstart(self, e):
        self._g = (e.x_root, e.y_root, *self.zone)

    def _gmove(self, e):
        gx, gy, zx, zy, zw, zh = self._g
        self.zone = [zx + e.x_root - gx, zy + e.y_root - gy, zw, zh]
        self.layout()
        self.on_change(tuple(self.zone))

    def close(self):
        for w in self.wins + [self.grip]:
            try:
                w.destroy()
            except Exception:
                pass
        self.wins = []


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

        self.scroll_iv = 1.0
        self.auto_scroll = tk.BooleanVar(value=False)
        row = tk.Frame(self.root, bg="#1e1e1e")
        row.pack(anchor="w", padx=10)
        tk.Checkbutton(row, text="Auto scroll", variable=self.auto_scroll, bg="#1e1e1e",
                       fg="#d0d0d0", selectcolor="#333333", activebackground="#1e1e1e",
                       activeforeground="#ffffff", font=("Consolas", 9)).pack(side="left")
        self._ivbtn = tk.Button(row, text="1.0s", command=self.cycle_iv, bg="#3d3d3d", fg="#a0d0a0",
                                font=("Consolas", 8), width=5)
        self._ivbtn.pack(side="left", padx=4)
        self.border_var = tk.BooleanVar(value=True)
        tk.Checkbutton(row, text="Show border", variable=self.border_var,
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
        self.root.geometry(f"+{max(0, sw - pw - 5)}+{max(0, sh - ph - 150)}")
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
            self.copy_path()
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
