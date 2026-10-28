"""Fit Greg's icon art (src/original.png) onto the macOS icon grid as icons/source.png.

The art is a full-bleed rounded square; macOS wants an 824 px body centred on a
1024 px canvas with a soft shadow below. Run from app/: python3 icons/src/make-source.py
"""

from PIL import Image, ImageFilter

art = Image.open("icons/src/original.png").convert("RGBA")
# Crop to the rounded square (alpha > 5) and square it up to the 824 px body.
left, top, right, bottom = art.getchannel("A").point(lambda v: 255 if v > 5 else 0).getbbox()
body = art.crop((left, top, right, bottom)).resize((826, 826), Image.LANCZOS)
r, g, b, a = body.split()
a = a.point(lambda v: 255 if v >= 235 else v)  # the art is ~1% see-through; make the body opaque
body = Image.merge("RGBA", (r, g, b, a))

canvas = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
shadow = Image.new("L", (1024, 1024), 0)
shadow.paste(a, (99, 109))
shadow = shadow.filter(ImageFilter.GaussianBlur(14)).point(lambda v: int(v * 0.35))
canvas.putalpha(shadow)
canvas.alpha_composite(body, (99, 99))
canvas.save("icons/source.png")
