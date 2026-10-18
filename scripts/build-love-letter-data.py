"""Reproduce the Love Letter GB2312 stroke resource (Python standard library only).

Upstream graphics are Arphic Public License data, separate from the dictionary.
No dictionary or runtime network dependency is included.
"""
import argparse
import hashlib
import json
import re
import struct
import zlib
from pathlib import Path
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]
REPO = "https://raw.githubusercontent.com/skishore/makemeahanzi"
MAGIC = b"NTHZ\x00\x00\x00\x01"
BLOCK_SIZE = 64
COMMANDS = "MLQCZ"
ARGUMENTS = (2, 2, 4, 6, 0)


def varint(out, value):
    while value >= 128:
        out.append((value & 127) | 128)
        value >>= 7
    out.append(value)


def delta(out, value):
    varint(out, value * 2 if value >= 0 else -value * 2 - 1)


def encode_glyph(glyph):
    out = bytearray()
    assert len(glyph["strokes"]) == len(glyph["medians"])
    varint(out, len(glyph["strokes"]))
    for path, median in zip(glyph["strokes"], glyph["medians"]):
        # The pinned upstream subset uses absolute MLQCZ and integral outlines.
        tokens = re.findall(r"[MLQCZ]|-?\d+", path)
        assert "".join(tokens) == re.sub(r"\s+", "", path), path
        segments = []
        position = 0
        while position < len(tokens):
            command = COMMANDS.index(tokens[position])
            count = ARGUMENTS[command]
            coordinates = list(map(int, tokens[position + 1:position + 1 + count]))
            assert len(coordinates) == count
            segments.append((command, coordinates))
            position += count + 1
        varint(out, len(segments))
        previous = [0, 0]
        for command, coordinates in segments:
            out.append(command)
            for axis, coordinate in enumerate(coordinates):
                delta(out, coordinate - previous[axis % 2])
                previous[axis % 2] = coordinate
        varint(out, len(median))
        previous = [0, 0]
        for point in median:
            for axis, coordinate in enumerate(point):
                fixed = int(coordinate * 4)
                assert fixed == coordinate * 4, "Coordinate is not a quarter unit"
                delta(out, fixed - previous[axis])
                previous[axis] = fixed
    return bytes(out)


def pack(document):
    metadata = {key: value for key, value in document.items() if key != "characters"}
    encoded_meta = json.dumps(metadata, ensure_ascii=False, separators=(",", ":")).encode()
    glyphs = [(ord(char), encode_glyph(glyph))
              for char, glyph in sorted(document["characters"].items())]
    index = bytearray()
    table = bytearray()
    payload = bytearray()
    for start in range(0, len(glyphs), BLOCK_SIZE):
        raw = bytearray()
        for codepoint, glyph in glyphs[start:start + BLOCK_SIZE]:
            index.extend(struct.pack("<III", codepoint, len(raw), len(glyph)))
            raw.extend(glyph)
        compressed = zlib.compress(raw, 9)
        table.extend(struct.pack("<III", len(payload), len(compressed), len(raw)))
        payload.extend(compressed)
    header = MAGIC + struct.pack("<IIII", len(encoded_meta), len(glyphs),
                                (len(glyphs) + BLOCK_SIZE - 1) // BLOCK_SIZE, BLOCK_SIZE)
    return header + encoded_meta + index + table + payload


def build(revision: str, output: Path):
    document = source_document(revision)
    output.mkdir(parents=True, exist_ok=True)
    encoded = pack(document)
    (output / "strokes.nthz").write_bytes(encoded)
    (output / "COPYING").write_bytes(urlopen(f"{REPO}/{revision}/COPYING").read())
    for language in ("english", "zh_CN.UTF-8", "zh_TW.UTF-8"):
        licence = urlopen(f"{REPO}/{revision}/APL/{language}/ARPHICPL.TXT").read()
        if language == "english":
            (output / "ARPHICPL.TXT").write_bytes(licence)
        else:
            (output / f"APL-{language}.txt").write_bytes(licence)
    (output / "SOURCES.txt").write_text(
        f"Make Me a Hanzi, revision {revision}\nSource graphics SHA-256: {document['source_sha256']}\n"
        f"{len(document['characters'])} characters: GB2312 hanzi intersected with graphics.txt.\n"
        "Modified 2026-10-18 by NanaType: subset; Unicode index; independent 64-character zlib blocks; "
        "lossless command and delta coordinate encoding (quarter-unit medians). Geometry and stroke order unchanged.\n"
        "Graphics and derived stroke masks: Arphic Public License (see ARPHICPL.TXT and APL-*.txt).\n"
        "No dictionary data is distributed. Font is independently licensed under SIL OFL 1.1.\n",
        encoding="utf-8", newline="\n",
    )
    print(f"{len(document['characters'])} characters, {len(encoded):,} bytes -> {output}")


def source_document(revision):
    raw = urlopen(f"{REPO}/{revision}/graphics.txt").read()
    allowed = {
        bytes([hi, lo]).decode("gb2312", errors="ignore")
        for hi in range(0xB0, 0xF8) for lo in range(0xA1, 0xFF)
    } - {""}
    characters = {}
    for line in raw.splitlines():
        row = json.loads(line)
        if row["character"] in allowed:
            characters[row["character"]] = {
                "strokes": row["strokes"], "medians": row["medians"]
            }
    return {
        "format": 2,
        "notice": (
            "Modified by NanaType on 2026-10-18: GB2312 subset; character index; "
            "64-character zlib blocks; lossless command and delta coordinate encoding. "
            "Original geometry and stroke order retained. Arphic Public License."
        ),
        "source": f"https://github.com/skishore/makemeahanzi/tree/{revision}",
        "source_sha256": hashlib.sha256(raw).hexdigest(),
        "characters": dict(sorted(characters.items())),
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--revision", default="bddc96d41bef78427ed0e034e9f7e31d71fd1b92")
    parser.add_argument("--output", type=Path, default=ROOT / "crates/theme/builtin/love_letter")
    args = parser.parse_args()
    build(args.revision, args.output)
