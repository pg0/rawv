"""Draws the rawv icon, a piece of film with an accent-coloured picture, into assets/rawv.ico.

Usage: python tools/make_icon.py [--preview PNG]   (needs pillow)

The icon file is the only source of the icon: build.rs puts it into the executables and the
window loads it from there. Small sizes get fewer, larger perforations so they stay readable.
"""
import argparse
from pathlib import Path

from PIL import Image, ImageDraw

REPO = Path(__file__).resolve().parent.parent
TARGET = REPO / "assets" / "rawv.ico"
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SUPERSAMPLE = 8

FILM = (0x1C, 0x1C, 0x1C, 0xFF)
RIM = (0x4A, 0x4A, 0x4A, 0xFF)
PICTURE = (0xE2, 0xB7, 0x3A, 0xFF)
CLEAR = (0, 0, 0, 0)


def holes_per_row(size):
    if size <= 16:
        return 3
    if size <= 24:
        return 4
    if size <= 40:
        return 5
    return 7


def draw(size):
    """The icon at `size` pixels, drawn on a 64-unit grid and scaled down smoothly."""
    canvas = size * SUPERSAMPLE
    unit = canvas / 64
    image = Image.new("RGBA", (canvas, canvas), CLEAR)
    pen = ImageDraw.Draw(image)

    def box(left, top, right, bottom):
        return [round(left * unit), round(top * unit), round(right * unit) - 1, round(bottom * unit) - 1]

    pen.rounded_rectangle(box(2, 6, 62, 58), radius=3 * unit, fill=RIM)
    rim = 1.5 if size >= 32 else 2.5
    pen.rounded_rectangle(box(2 + rim, 6 + rim, 62 - rim, 58 - rim), radius=2 * unit, fill=FILM)
    pen.rectangle(box(8, 20, 56, 44), fill=PICTURE)

    count = holes_per_row(size)
    pitch = 54 / count
    width = pitch * 5 / 9
    for row_top, row_bottom in ((9.5, 16), (48, 54.5)):
        for index in range(count):
            left = 5 + index * pitch + (pitch - width) / 2
            pen.rectangle(box(left, row_top, left + width, row_bottom), fill=CLEAR)
    return image.resize((size, size), Image.LANCZOS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--preview", help="also write all sizes side by side to this PNG")
    args = parser.parse_args()
    images = [draw(size) for size in SIZES]
    TARGET.parent.mkdir(exist_ok=True)
    largest = images[-1]
    largest.save(TARGET, format="ICO", sizes=[(s, s) for s in SIZES], append_images=images[:-1])
    print(f"{TARGET} {TARGET.stat().st_size} bytes, sizes {SIZES}")
    if args.preview:
        gap = 12
        sheet = Image.new("RGBA", (sum(SIZES) + gap * (len(SIZES) + 1), 256 + 2 * gap), (0x20, 0x20, 0x20, 0xFF))
        light = Image.new("RGBA", sheet.size, (0xF3, 0xF3, 0xF3, 0xFF))
        x = gap
        for size, image in zip(SIZES, images):
            sheet.alpha_composite(image, (x, gap))
            light.alpha_composite(image, (x, gap))
            x += size + gap
        both = Image.new("RGBA", (sheet.width, sheet.height * 2))
        both.paste(sheet, (0, 0))
        both.paste(light, (0, sheet.height))
        both.save(args.preview)


if __name__ == "__main__":
    main()
