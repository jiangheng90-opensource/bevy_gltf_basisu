# bevy_gltf_basisu

Bevy plugin providing [`KHR_texture_basisu`](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_texture_basisu)
support for the glTF loader.

The plugin registers a [`GltfExtensionHandler`] that claims glTF textures
carrying the `KHR_texture_basisu` extension and transcodes their KTX2
Basis Universal payloads (ETC1S / UASTC, including zstd-supercompressed
levels) with [`basis_transcoder`](https://crates.io/crates/basis_transcoder),
picking the best GPU compressed format available (BC7 / ETC2 / ASTC, with an
uncompressed RGBA8 fallback). It works on native and WebAssembly through one
API — the same pattern as [`bevy_gltf_draco`](https://crates.io/crates/bevy_gltf_draco).

## Requirements

This plugin is built on the `on_texture_load` glTF extension hook, which is
not yet in a bevy release — see
[bevyengine/bevy#25669](https://github.com/bevyengine/bevy/pull/25669).
Until it lands, the bevy dependency must point at that branch.

Textures that omit the standard `source` (allowed by `KHR_texture_basisu`)
fail glTF validation, so load glTFs with `GltfLoaderSettings::validate`
set to `false`.

## Usage

```toml
[dependencies]
bevy = { git = "https://github.com/jiangheng90-opensource/bevy.git", branch = "gltf-texture-load-hook" }
bevy_gltf_basisu = { git = "https://github.com/jiangheng90-opensource/bevy_gltf_basisu.git" }
```

```rust
use bevy::prelude::*;
use bevy_gltf_basisu::GltfBasisuDecoderPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GltfBasisuDecoderPlugin)
        .run();
}
```

```rust
// Textures without a standard `source` need validation disabled:
asset_server
    .load_builder()
    .with_settings(|settings: &mut GltfLoaderSettings| settings.validate = false)
    .load(GltfAssetLabel::Scene(0).from_asset("models/StainedGlassLamp/StainedGlassLamp.gltf"));
```

## How it works

1. For every glTF texture, the handler checks for the
   `KHR_texture_basisu` extension and resolves the image reference stored in
   the extension data (the standard `texture.source` may be absent).
2. Image bytes are read from a buffer view, a data URI, or an external URI.
3. KTX2 payloads are transcoded via `basis_transcoder`:
   - native: official C++ transcoder over `cxx`, target picked from the
     GPU's supported compressed formats (BC7 > ETC2 > ASTC > RGBA8);
   - wasm: the official prebuilt Emscripten transcoder running in an inline
     Web Worker, targeting the reported compressed formats with an RGBA8
     fallback.
4. Textures without the extension fall through to bevy's default loading,
   unchanged (and still parallel).

## Examples and tests

```sh
cargo run --example main --release   # loads the StainedGlassLamp sample model
cargo test                           # integration tests with real assets
npm start                            # build and serve the wasm example
```

Test assets include an ETC1S texture extracted from a real Cesium ion
3D Tiles service and the Khronos
[StainedGlassLamp](https://github.com/KhronosGroup/glTF-Sample-Models/tree/main/2.0/StainedGlassLamp)
sample model (`glTF-KTX-BasisU` variant, 19 basisu textures). See
[assets/README.md](assets/README.md) for asset licenses.

## License

MIT OR Apache-2.0
