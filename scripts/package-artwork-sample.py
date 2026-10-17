"""Package the minimal, standalone procedural artwork example (stdlib only)."""
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

root = Path(__file__).resolve().parents[1]
sources = root / "crates/theme/builtin"
manifest = (sources / "celestial.toml").read_text(encoding="utf-8")
manifest = manifest.replace('id = "nanatype.celestial"', 'id = "example.lunar_atlas"')
entries = {"skin.toml": manifest.encode("utf-8")}
for path in ("celestial/artwork.wgsl", "celestial/still.png", "celestial/ornament.png"):
    entries[path] = (sources / path).read_bytes()
with ZipFile(root / "docs/examples/lunar-atlas.ntskin", "w") as archive:
    for path, data in entries.items():
        info = ZipInfo(path, (1980, 1, 1, 0, 0, 0))
        info.compress_type = ZIP_DEFLATED
        archive.writestr(info, data)
