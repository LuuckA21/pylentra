#!/usr/bin/env python3
# Dev tool. Requires: pip install cairosvg pillow
# Run after `pnpm tauri icon src-tauri/icons/source/icon.svg`.
import io
from pathlib import Path

import cairosvg
from PIL import Image

icons = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def render(svg, size):
    png = cairosvg.svg2png(url=str(icons / "source" / svg), output_width=size, output_height=size)
    return Image.open(io.BytesIO(png)).convert("RGBA")


small = [render("icon-small.svg", s) for s in (16, 24, 32)]
large = [render("icon.svg", s) for s in (48, 64, 128, 256)]
small[2].save(icons / "32x32.png")
# Pillow's ICO writer takes the frames from append_images, matched by size.
frames = small + large
frames[-1].save(
    icons / "icon.ico",
    format="ICO",
    sizes=[f.size for f in frames],
    append_images=frames[:-1],
)
