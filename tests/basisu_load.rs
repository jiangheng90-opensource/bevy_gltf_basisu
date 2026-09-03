//! Integration test: a glTF whose texture uses `KHR_texture_basisu` (with
//! the image reference in the extension and no standard `texture.source`)
//! is loaded through the `GltfBasisuDecoderPlugin` extension handler.
//!
//! The sample is an ETC1S (BasisLZ) texture atlas extracted from a real
//! Cesium ion Japan 3D city b3dm tile (asset 2602291).

use std::path::Path;

use base64::Engine as _;
use bevy::app::{App, TaskPoolPlugin};
use bevy::asset::{
    AssetApp, AssetPlugin, AssetServer, Assets, LoadState,
    io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
};
use bevy::gltf::{Gltf, GltfLoaderSettings, GltfMaterial, GltfPlugin};
use bevy::image::Image;
use bevy::log::LogPlugin;
use bevy::mesh::MeshPlugin;
use bevy::world_serialization::WorldSerializationPlugin;
use bevy_gltf_basisu::GltfBasisuDecoderPlugin;
use wgpu_types::TextureFormat;

const SAMPLE: &[u8] = include_bytes!("../assets/etc1s_sample.ktx2");

const LARGE_ITERATION_COUNT: usize = 10000;

fn run_app_until(app: &mut App, mut predicate: impl FnMut() -> bool) {
    for _ in 0..LARGE_ITERATION_COUNT {
        app.update();
        if predicate() {
            return;
        }
    }
    panic!("Ran out of loops waiting for the predicate");
}

#[test]
fn load_gltf_with_khr_texture_basisu() {
    let ktx2_base64 = base64::engine::general_purpose::STANDARD.encode(SAMPLE);
    let gltf_json = format!(
        r#"{{
    "asset": {{ "version": "2.0" }},
    "extensionsUsed": ["KHR_texture_basisu"],
    "extensionsRequired": ["KHR_texture_basisu"],
    "scene": 0,
    "scenes": [{{ "nodes": [0] }}],
    "nodes": [{{ "name": "root" }}],
    "buffers": [{{
        "byteLength": {},
        "uri": "data:application/octet-stream;base64,{}"
    }}],
    "bufferViews": [{{ "buffer": 0, "byteOffset": 0, "byteLength": {} }}],
    "images": [{{ "bufferView": 0, "mimeType": "image/ktx2" }}],
    "textures": [
        {{ "extensions": {{ "KHR_texture_basisu": {{ "source": 0 }} }} }}
    ],
    "materials": [
        {{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }} }}
    ]
}}"#,
        SAMPLE.len(),
        ktx2_base64,
        SAMPLE.len(),
    );

    let gltf_path = "test.gltf";
    let dir = Dir::default();
    dir.insert_asset_text(Path::new(gltf_path), &gltf_json);

    let reader = MemoryAssetReader { root: dir };
    let mut app = App::new();
    app.register_asset_source(
        AssetSourceId::Default,
        AssetSourceBuilder::new(move || Box::new(reader.clone())),
    )
    .add_plugins((
        LogPlugin::default(),
        TaskPoolPlugin::default(),
        AssetPlugin::default(),
        MeshPlugin,
        WorldSerializationPlugin,
        GltfPlugin::default(),
        GltfBasisuDecoderPlugin,
    ));
    app.init_asset::<Image>();
    app.finish();
    app.cleanup();

    app.update();
    let asset_server = app.world().resource::<AssetServer>().clone();
    // Textures that omit the standard `source` fail glTF validation.
    let handle: bevy::prelude::Handle<Gltf> = asset_server
        .load_builder()
        .with_settings(|settings: &mut GltfLoaderSettings| settings.validate = false)
        .load(gltf_path.to_string());
    let handle_id = handle.id();
    app.update();
    run_app_until(&mut app, || {
        match asset_server.get_load_state(handle_id).unwrap() {
            LoadState::Loaded => true,
            LoadState::Failed(err) => panic!("{err}"),
            _ => false,
        }
    });

    let gltf_root_assets = app.world().resource::<Assets<Gltf>>();
    let gltf_root = gltf_root_assets.get(&handle).unwrap();
    let materials = app.world().resource::<Assets<GltfMaterial>>();
    let images = app.world().resource::<Assets<Image>>();

    let material = materials.get(&gltf_root.materials[0]).unwrap();
    let texture_handle = material.base_color_texture.as_ref().unwrap();
    let image = images.get(texture_handle).unwrap();

    // Without render device capabilities the handler transcodes to RGBA8.
    let desc = &image.texture_descriptor;
    assert_eq!((desc.size.width, desc.size.height), (16, 28));
    assert_eq!(desc.mip_level_count, 5);
    assert_eq!(desc.format, TextureFormat::Rgba8UnormSrgb);

    let data = image.data.as_ref().unwrap();
    let mut expected = 0;
    for level in 0..5u32 {
        expected += (16usize >> level).max(1) * (28usize >> level).max(1) * 4;
    }
    assert_eq!(data.len(), expected);

    // Level 0 must not decode to a solid color.
    let first = &data[0..4];
    assert!(
        data[..16 * 28 * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p != first)
    );
}
