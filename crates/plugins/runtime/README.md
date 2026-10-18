# Pinned interpreter

`lua.wasm` is the Lua interpreter distributed by [Wasmoon 1.16.0](https://github.com/ceifa/wasmoon/tree/87c783a8d0bf65afeda933fa47d8dc996be53bb3), from the npm registry tarball `https://registry.npmjs.org/wasmoon/-/wasmoon-1.16.0.tgz`.

- npm archive SHA-512 (base64): `FlRLb15WwAOz1A9OQDbf6oOKKSiefi5VK0ZRF2wgH9xk3o5SnU11tNPaOnQuAh1Ucr66cwwvVXaeVRaFdRBt5g==`
- Wasm SHA-256: `95f3f19ddb740125883bc41d5ec670cd0828e7b58c8bdad385323cbff497b55c`
- Size: 271581 bytes.

The archive integrity was verified before extracting `package/dist/glue.wasm`. The application's interpreter is embedded at build time; packages cannot supply a replacement. No JavaScript glue or Wasmoon filesystem implementation is executed. Wasmtime's linker handles the necessary internal callback trampoline, exposes an empty environment, and traps filesystem/process imports. Only base, table, string, math and UTF-8 Lua libraries are opened; unsafe entry points are removed before author code runs.

Wasmoon is MIT licensed (`WASMOON-LICENSE`); Lua's copyright and MIT license are in `LUA-LICENSE`. Preserve both when redistributing. Rebuilding the binary requires the pinned upstream source and its Emscripten build, outside application startup. The normal Rust build verifies this artifact and precompiles it into an application-owned native Wasmtime module for TARGET, with fuel and epoch interruption enabled. Worker startup deserializes only these embedded build bytes, never a package-supplied artifact or writable filesystem cache. The runtime dependency excludes the compiler; the normal build performs no interpreter downloads.
