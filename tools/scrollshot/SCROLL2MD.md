# ScrollShot PNG → Markdown

How to convert a "sausage" PNG produced by ScrollShot into a readable `.md`
transcript. Works for any tall stitched screenshot.

## Pipeline

```
tall.png ──► slice into overlapping chunks ──► vision agents (parallel)
          ──► stitch transcripts in order ──► output.md
```

1. **Slice** — cut the PNG into ~1400px chunks with ~200px vertical overlap
   (overlap keeps split paragraphs/code blocks readable at seams):

```python
from PIL import Image
im = Image.open("scrollshot_XXXXXXXX.png")
w, h = im.size
step, ch = 1200, 1400          # 1200px stride, 1400px window
n = 0
for y in range(0, h, step):
    im.crop((0, y, w, min(y + ch, h))).save(f"/tmp/chunks/chunk_{n:02d}_{y}.png")
    n += 1
```

2. **Fan out** — give each agent 4–6 consecutive chunks (~6 agents for a
   35 000px image). Prompt per agent:

   > Transcribe these scrolling-screenshot chunks to markdown (RU/EN technical
   > chat log). Read each image fully, output ONE combined markdown transcript
   > in order, preserving headings/lists/code blocks/tables verbatim. Chunks
   > overlap ~200px — deduplicate repeated lines at seams. Return ONLY markdown.

   Agents: `run_subagent` (explore profile, read-only) — vision models read PNGs
   directly. Keep each agent's chunk ranges disjoint; seams deduplicated on
   stitch.

3. **Stitch** — concatenate returned markdown in chunk order; remove leftover
   duplicated lines between segment boundaries.

## Notes

- **Glitched start**: ScrollShot's first ~1400px can be black-banded on some
  runs — check chunk_00 visually; if corrupted, find the same content in a
  later chunk/single screenshot and substitute it for the head.
- **Output** goes next to the source PNG:
  `C:\Users\therceman\Pictures\Screenshots\ScrollShot\<name>.md`.
- A single agent can handle the whole thing sequentially but is slow —
  parallelism is the point of the fan-out.
- If the content is code/dense tables, keep chunks narrower (1100–1200px
  stride) so no row is cut at a seam without overlap backup.
