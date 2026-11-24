"""Copies a RAW file and disables its largest embedded JPEG, as if the camera had stored
only a small preview. rawv then has to develop the sensor data to show the full size.

Usage: python tools/small_preview_raw.py SOURCE_RAW TARGET_RAW
"""
import struct
import sys
from pathlib import Path


def jpeg_size(data, start):
    """Pixel size of the JPEG that starts at `start`, or None when it is not one."""
    pos = start + 2
    while pos + 4 <= len(data) and data[pos] == 0xFF:
        marker = data[pos + 1]
        length = struct.unpack_from(">H", data, pos + 2)[0]
        if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
            height, width = struct.unpack_from(">HH", data, pos + 5)
            return width, height
        if marker == 0xDA or length < 2:
            return None
        pos += 2 + length
    return None


def write_small_preview_raw(source, target):
    data = bytearray(Path(source).read_bytes())
    previews = []
    start = data.find(b"\xff\xd8\xff")
    while start >= 0:
        size = jpeg_size(data, start)
        if size:
            previews.append((size[0] * size[1], start, size))
        start = data.find(b"\xff\xd8\xff", start + 3)
    if len(previews) < 2:
        raise SystemExit(f"{source}: needs at least two embedded JPEGs, found {len(previews)}")
    previews.sort()
    _, largest, size = previews[-1]
    data[largest:largest + 4] = b"\0\0\0\0"
    Path(target).write_bytes(data)
    return size, previews[-2][2]


if __name__ == "__main__":
    disabled, remaining = write_small_preview_raw(sys.argv[1], sys.argv[2])
    print(f"disabled {disabled[0]}x{disabled[1]}, largest left {remaining[0]}x{remaining[1]}")
