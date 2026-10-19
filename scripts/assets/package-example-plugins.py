"""Build reproducible functional-plugin examples with the standard library."""
from pathlib import Path
from zipfile import ZIP_DEFLATED, ZipFile, ZipInfo

examples = Path(__file__).resolve().parents[2] / "docs/examples"
for directory in sorted(examples.glob("*-plugin")):
    target = directory.with_suffix(".ntpkg")
    with ZipFile(target, "w") as archive:
        for source in sorted(directory.rglob("*")):
            if source.is_file():
                info = ZipInfo(source.relative_to(directory).as_posix(), (1980, 1, 1, 0, 0, 0))
                info.compress_type = ZIP_DEFLATED
                archive.writestr(info, source.read_bytes())
    print(target)
