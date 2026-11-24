"""Builds the photo tree used by tools/verify.py: 24 MP JPEGs with EXIF thumbnails plus copies of a real RAW file.

Usage: python tools/make_test_photos.py OUT_DIR RAW_FILE SEED_JPEG   (needs: pip install pillow piexif)
"""
import io, random, shutil, sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
import piexif

from small_preview_raw import write_small_preview_raw

out = Path(sys.argv[1]); arw = Path(sys.argv[2]); seed_jpeg = Path(sys.argv[3])
random.seed(7)
base = Image.open(seed_jpeg).convert("RGB")

def photo(index, size, orientation):
    w, h = size
    img = Image.new("RGB", (w // 8, h // 8))
    draw = ImageDraw.Draw(img)
    hue = (index * 47) % 360
    for y in range(img.height):
        t = y / img.height
        draw.line([(0, y), (img.width, y)], fill=Image.new("HSV", (1, 1), (int(hue / 360 * 255), 150, int(60 + 180 * t))).convert("RGB").getpixel((0, 0)))
    for _ in range(14):
        x, y, r = random.randrange(img.width), random.randrange(img.height), random.randrange(20, 160)
        col = tuple(random.randrange(256) for _ in range(3))
        draw.ellipse([x - r, y - r, x + r, y + r], fill=col)
    crop = base.resize((img.width // 3, img.height // 2))
    img.paste(crop, (img.width // 10, img.height // 4))
    draw.text((img.width // 2, img.height // 2), f"#{index:03d}", fill=(255, 255, 255))
    img = img.resize((w, h), Image.BICUBIC)
    noise = Image.effect_noise((w, h), 22).convert("RGB")
    img = Image.blend(img, noise, 0.12)
    thumb = img.copy(); thumb.thumbnail((160, 160))
    canvas = Image.new("RGB", (160, 120)); canvas.paste(thumb, ((160 - thumb.width) // 2, (120 - thumb.height) // 2))
    buf = io.BytesIO(); canvas.save(buf, "JPEG", quality=80)
    exif = piexif.dump({
        "0th": {piexif.ImageIFD.Make: b"TestCam", piexif.ImageIFD.Model: b"TC-24", piexif.ImageIFD.Orientation: orientation},
        "Exif": {piexif.ExifIFD.ExposureTime: (1, 250), piexif.ExifIFD.FNumber: (28, 10), piexif.ExifIFD.ISOSpeedRatings: 200,
                 piexif.ExifIFD.FocalLength: (35, 1), piexif.ExifIFD.DateTimeOriginal: b"2026:05:%02d 10:%02d:00" % (1 + index % 28, index % 60),
                 piexif.ExifIFD.LensModel: b"35mm F1.8"},
        "1st": {piexif.ImageIFD.Compression: 6}, "thumbnail": buf.getvalue()})
    return img, exif

def write(folder, count, start, size=(6000, 4000)):
    folder.mkdir(parents=True, exist_ok=True)
    for i in range(count):
        n = start + i
        orientation = 6 if n % 7 == 3 else 1
        img, exif = photo(n, size, orientation)
        img.save(folder / f"IMG_{n:04d}.JPG", "JPEG", quality=92, exif=exif)

root = out / "shoot"
write(root, 9, 1)
for i in range(4): shutil.copy(arw, root / f"DSC0{i:04d}.ARW")
write(root / "2026-05 Berlin", 40, 100)
write(root / "2026-06 Wedding", 14, 200)
(root / "raw only").mkdir(parents=True, exist_ok=True)
for i in range(6): shutil.copy(arw, root / "raw only" / f"DSC1{i:04d}.ARW")
(root / "empty").mkdir(exist_ok=True)
write(root / "archive" / "2019", 3, 300, (3000, 2000))
(root / "small preview").mkdir(exist_ok=True)
write_small_preview_raw(arw, root / "small preview" / "DSC20000.ARW")
base.save(root / "portrait_noexif.jpg", quality=90)
Image.new("RGBA", (800, 600), (40, 120, 200, 255)).save(root / "graphic.png")
print("done")
