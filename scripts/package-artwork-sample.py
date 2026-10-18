"""Package the minimal, standalone procedural artwork example (stdlib only)."""
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

root = Path(__file__).resolve().parents[1]
sources = root / "crates/theme/builtin"
package = sources / "celestial"
metadata = (package / "package.toml").read_text(encoding="utf-8")
metadata = metadata.replace('id = "nanatype.celestial"', 'id = "example.lunar_atlas"')
entries = {path.relative_to(package).as_posix(): path.read_bytes() for path in package.rglob('*') if path.is_file()}
entries["package.toml"] = metadata.encode("utf-8")
with ZipFile(root / "docs/examples/lunar-atlas.ntpkg", "w") as archive:
    for path, data in entries.items():
        info = ZipInfo(path, (1980, 1, 1, 0, 0, 0))
        info.compress_type = ZIP_DEFLATED
        archive.writestr(info, data)
