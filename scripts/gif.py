"""Joins PNG frames into an optimized GIF: python3 scripts/gif.py out.gif a.png b.png ...

Every frame gets its own octree palette, so small colours (syntax
highlighting) survive. Needs Pillow. WIDTH and SECONDS tune size and pace.
"""

import os
import sys

from PIL import Image

WIDTH = int(os.environ.get("WIDTH", "1400"))
SECONDS = float(os.environ.get("SECONDS", "2.2"))


def main(out, paths):
    frames = []
    for path in paths:
        image = Image.open(path).convert("RGB")
        height = round(image.height * WIDTH / image.width)
        frames.append(image.resize((WIDTH, height), Image.LANCZOS))
    # A palette per frame, by octree: median cut on one shared palette drops
    # the small syntax colours of a diff for the big dark grounds.
    quantized = [f.quantize(colors=256, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE) for f in frames]
    quantized[0].save(
        out,
        save_all=True,
        append_images=quantized[1:],
        duration=int(SECONDS * 1000),
        loop=0,
        optimize=True,
    )
    print(f"{out}: {len(frames)} frames, {os.path.getsize(out) / 1e6:.1f} MB")


if __name__ == "__main__":
    assert len(sys.argv) > 2, __doc__
    main(sys.argv[1], sys.argv[2:])
