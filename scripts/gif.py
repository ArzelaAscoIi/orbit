"""Joins PNG frames into an optimized GIF: python3 scripts/gif.py out.gif a.png b.png ...

Every frame shares one palette, so the GIF stays small and the colours hold
from frame to frame. Needs Pillow. WIDTH and SECONDS tune size and pace.
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
    # One palette from all frames side by side.
    strip = Image.new("RGB", (WIDTH, sum(f.height for f in frames)))
    y = 0
    for frame in frames:
        strip.paste(frame, (0, y))
        y += frame.height
    palette = strip.quantize(colors=256, method=Image.Quantize.MEDIANCUT)
    quantized = [f.quantize(palette=palette, dither=Image.Dither.NONE) for f in frames]
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
