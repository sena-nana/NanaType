# Licence texts the crates' packages leave out

`scripts/third-party-notices.ps1 -Texts` (Windows) and `scripts/macos-licenses.py`
(macOS) write each linked Rust crate's own `LICENSE*`/`COPYING*`/`NOTICE*` files
into the `rust-crates.txt` section of `THIRD_PARTY_LICENSES.txt`. Some crates
publish no such file, or keep the licence of the C/C++ code they compile in a
subfolder. [crates.json](crates.json) adds those texts, by crate name (every
version of that name):

- `crate`: paths inside the crate's own source folder, such as a vendored SDK's
  licence. A missing path fails the run, so a dependency update that moves it
  is noticed.
- `upstream`: files in this folder, copied from the crate's upstream repository
  because the published package has none.

| File | Source |
| --- | --- |
| `objc2-LICENSE.md` | <https://github.com/madsmtm/objc2/blob/objc2-0.6.4/LICENSE.md> |
| `objc2-LICENSE-MIT.txt` | <https://github.com/madsmtm/objc2/blob/main/LICENSE-MIT.txt> (the MIT crates' terms; the repository added the text later) |
| `ebml-iterable-LICENSE.txt` | <https://github.com/austinleroy/ebml-iterable/blob/master/LICENSE> |
| `iKeySort-LICENSE.txt` | <https://github.com/iShape-Rust/iKeySort/blob/main/LICENSE> |
| `realfft-LICENSE.txt` | <https://github.com/HEnquist/realfft/blob/master/LICENSE> |
| `Symphonia-LICENSE.txt` | <https://github.com/pdeljanov/Symphonia/blob/v0.5.5/LICENSE> (MPL-2.0) |
| `tabler-icons-LICENSE.txt` | <https://github.com/tabler/tabler-icons/blob/v3.46.0/LICENSE> (the icons in `nana-icons-tabler`) |
| `webview2-rs-LICENSE.txt` | <https://github.com/wravery/webview2-rs/blob/main/LICENSE> |

Both scripts fail when a linked crate has no licence text and its licence
expression offers no term that needs none of its own: `Apache-2.0` and
`Apache-2.0 WITH LLVM-exception` (the full text already ships with many other
crates), `BSL-1.0` (no notice for
machine-executable code), `Unlicense`, `CC0-1.0`. Add the missing text here
when that happens.
