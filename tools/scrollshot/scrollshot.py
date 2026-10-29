"""ScrollShot: select a screen zone, scroll once per frame, stitch into one long PNG."""
import argparse, datetime, os, sys, threading, time
import numpy as np
from PIL import Image

IDLE_LIMIT = 3          # stop after N frames with no change
SETTLE_MS = 300         # wait after wheel event before capture
MIN_OVERLAP = 0.25      # at least 25% of height must overlap
MSE_LIMIT = 12.0        # mean abs diff threshold for "same content"


def select_zone():
    import tkinter as tk
    root = tk.Tk()
    root.attributes("-fullscreen", True)
    root.attributes("-alpha", 0.25)
    root.attributes("-topmost", True)
    root.configure(bg="black")
    cv = tk.Canvas(root, cursor="cross", bg="black", highlightthickness=0)
    cv.pack(fill="both", expand=True)
    state = {"x0": 0, "y0": 0, "rect": None}
    info = cv.create_text(20, 20, anchor="nw", fill="#00ff88", font=("Consolas", 16),
                          text="Drag to select zone   ·   Esc = cancel")

    def on_press(e):
        state["x0"], state["y0"] = e.x_root, e.y_root
        if state["rect"]:
            cv.delete(state["rect"])
        state["rect"] = cv.create_rectangle(e.x_root, e.y_root, e.x_root, e.y_root, outline="#00ff88", width=2)

    def on_drag(e):
        if state["rect"]:
            cv.coords(state["rect"], state["x0"], state["y0"], e.x_root, e.y_root)

    def on_release(e):
        x1, y1 = e.x_root, e.y_root
        x, y = min(state["x0"], x1), min(state["y0"], y1)
        w, h = abs(x1 - state["x0"]), abs(y1 - state["y0"])
        if w > 30 and h > 30:
            state["zone"] = (x, y, w, h)
            root.destroy()

    cv.bind("<ButtonPress-1>", on_press)
    cv.bind("<B1-Motion>", on_drag)
    cv.bind("<ButtonRelease-1>", on_release)
    root.bind("<Escape>", lambda e: root.destroy())
    root.mainloop()
    return state.get("zone")


def grab(zone):
    import mss
    x, y, w, h = zone
    with mss.mss() as sct:
        shot = sct.grab({"left": x, "top": y, "width": w, "height": h})
    return np.array(Image.frombytes("RGB", shot.size, shot.bgra, "raw", "BGRX"))


def sig2(img, cols=32):
    """Row signatures: downsample width to `cols` blocks -> (h, cols) float array."""
    g = img.mean(axis=2)
    h, w = g.shape
    cols = max(4, min(cols, w))
    step = max(1, w // cols)
    usable = cols * step
    g2 = g[:, :usable].reshape(h, cols, step).mean(axis=2)
    return g2


def find_shift(prev_sig, new_sig, verify=None):
    """Find s>0 so that prev_sig[s:] ~= new_sig[:h-s]. Returns (s, score)."""
    h = len(prev_sig)
    lo = int(h * MIN_OVERLAP)
    best_s, best_score = 0, float("inf")
    for s in range(1, h - lo + 1):
        a = prev_sig[s:]
        b = new_sig[:len(a)]
        score = float(np.abs(a - b).mean())
        if score < best_score:
            best_score, best_s = score, s
    if verify is not None and best_s > 0:
        full = verify(best_s)
        if full is not None:
            return best_s, full
    return best_s, best_score


def full_score(prev_img, new_img, s, rows_step=4):
    """Full-resolution MSE over the overlap, sampled rows (robust vs false positives)."""
    h = prev_img.shape[0]
    a = prev_img[s:h:rows_step].astype(np.int16)
    b = new_img[:h - s:rows_step].astype(np.int16)
    n = min(len(a), len(b))
    if n == 0:
        return None
    return float(np.abs(a[:n] - b[:n]).mean())


def best_shift(prev_img, new_img):
    ps, ns = sig2(prev_img), sig2(new_img)
    s, score = find_shift(ps, ns, verify=lambda s: full_score(prev_img, new_img, s))
    return s, score


def contains(canvas_img, new_img, tail_rows=None):
    """True if new_img already appears in the bottom part of canvas (duplicate content)."""
    ch = canvas_img.shape[0]
    nh = new_img.shape[0]
    if nh >= ch:
        return False
    lo = max(0, ch - nh - 200)
    ns = sig2(new_img)
    cs = sig2(canvas_img[lo:])
    best, best_score = None, float("inf")
    for off in range(0, len(cs) - len(ns) + 1):
        d = float(np.abs(cs[off:off + len(ns)] - ns).mean())
        if d < best_score:
            best_score, best = d, off
    return best_score < MSE_LIMIT


def run_capture(zone, out_path):
    frames = [grab(zone)]
    print(f"zone={zone} first frame {frames[0].shape[1]}x{frames[0].shape[0]}", flush=True)

    state = {"pending": False, "stop": False, "idle": 0}
    from pynput import mouse

    def on_scroll(x, y, dx, dy):
        state["pending"] = True

    listener = mouse.Listener(on_scroll=on_scroll)
    listener.start()
    print("scroll to capture · 3 idle frames = stop · Esc = finish", flush=True)

    try:
        while not state["stop"]:
            time.sleep(0.05)
            if not state["pending"]:
                continue
            state["pending"] = False
            time.sleep(SETTLE_MS / 1000.0)
            state["pending"] = False
            new = grab(zone)
            s, score = best_shift(frames[-1], new)
            changed = score < MSE_LIMIT and s > 0
            if changed:
                frames.append(new)
                state["idle"] = 0
                print(f"+ frame {len(frames)}  shift={s}px  score={score:.2f}", flush=True)
            else:
                state["idle"] += 1
                print(f"= no change ({state['idle']}/{IDLE_LIMIT})  shift={s} score={score:.2f}", flush=True)
                if state["idle"] >= IDLE_LIMIT:
                    state["stop"] = True
    except KeyboardInterrupt:
        pass
    finally:
        listener.stop()

    # stitch: append only the genuinely new bottom part, skip duplicates
    base = frames[0]
    parts = [base]
    total = base.shape[0]
    canvas_so_far = base
    for i in range(1, len(frames)):
        s, _ = best_shift(frames[i - 1], frames[i])
        if s <= 0:
            continue
        new_part = frames[i][base.shape[0] - s:]
        if new_part.shape[0] == 0:
            continue
        if contains(canvas_so_far, frames[i]):
            print(f"skip duplicate frame {i + 1}", flush=True)
            continue
        parts.append(new_part)
        total += s
        canvas_so_far = frames[i]
    canvas = np.zeros((total, base.shape[1], 3), dtype=np.uint8)
    y = 0
    for p in parts:
        canvas[y:y + p.shape[0]] = p
        y += p.shape[0]
    img = Image.fromarray(canvas)
    img.save(out_path)
    print(f"saved {out_path}  {img.width}x{img.height}  frames={len(frames)}", flush=True)
    return out_path


def selftest():
    rng = np.random.default_rng(0)
    tall = (rng.random((1200, 200, 3)) * 255).astype(np.uint8)
    tall[::40] = 30  # horizontal "text" lines
    frames = [tall[i:i + 400].copy() for i in range(0, 800, 120)]
    shifts = []
    for i in range(1, len(frames)):
        s, sc = best_shift(frames[i - 1], frames[i])
        shifts.append((s, round(float(sc), 2)))
    print("expected shift=120 for each step:", shifts)
    assert all(abs(s - 120) <= 1 for s, _ in shifts), "shift detection failed"
    # stitch and compare with the original tall image
    parts = [frames[0]]
    total = frames[0].shape[0]
    for i in range(1, len(frames)):
        s, _ = best_shift(frames[i - 1], frames[i])
        parts.append(frames[i][frames[0].shape[0] - s:])
        total += s
    canvas = np.zeros((total, frames[0].shape[1], 3), dtype=np.uint8)
    y = 0
    for p in parts:
        canvas[y:y + p.shape[0]] = p
        y += p.shape[0]
    ref = tall[:total]
    diff = float(np.abs(canvas.astype(np.int16) - ref.astype(np.int16)).mean())
    print(f"stitched {canvas.shape} vs original {ref.shape} diff={diff:.3f}")
    assert diff < 1.0, "stitching produced mismatched content"
    print("selftest OK (no duplicated content)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--selftest", action="store_true")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()
    if args.selftest:
        selftest()
        return
    zone = select_zone()
    if not zone:
        print("cancelled")
        return
    out = args.out or os.path.join(os.path.expanduser("~"), "Pictures", "Screenshots",
                                   f"scrollshot_{datetime.datetime.now():%Y%m%d_%H%M%S}.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    run_capture(zone, out)


if __name__ == "__main__":
    main()
