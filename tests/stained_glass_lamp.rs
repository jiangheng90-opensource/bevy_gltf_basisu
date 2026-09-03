//! Integration test loading the full Khronos `StainedGlassLamp` sample model
//! (`glTF-KTX-BasisU` variant, CC-BY 4.0 Wayfair — see `assets/README.md`).
//! All 19 of its textures use `KHR_texture_basisu`, covering ETC1S and UASTC
//! KTX2, sRGB and linear, and zstd-supercompressed levels.

use std::path::Path;

use bevy::app::{App, TaskPoolPlugin};
use bevy::asset::{
    AssetApp, AssetPlugin, AssetServer, Assets, LoadState,
    io::{
        AssetSourceBuilder, AssetSourceId,
        memory::{Dir, MemoryAssetReader},
    },
};
use bevy::gltf::{Gltf, GltfLoaderSettings, GltfPlugin};
use bevy::image::Image;
use bevy::log::LogPlugin;
use bevy::mesh::MeshPlugin;
use bevy::world_serialization::WorldSerializationPlugin;
use bevy_gltf_basisu::GltfBasisuDecoderPlugin;

const LARGE_ITERATION_COUNT: usize = 10000;
const MODEL_DIR: &str = "assets/models/StainedGlassLamp";

#[test]
fn load_stained_glass_lamp() {
    // Mirror the model directory into the in-memory asset source.
    let dir = Dir::default();
    let model_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_DIR);
    for entry in std::fs::read_dir(&model_path).unwrap() {
        let entry = entry.unwrap();
        let bytes = std::fs::read(entry.path()).unwrap();
        dir.insert_asset(Path::new(entry.file_name().to_str().unwrap()), bytes);
    }

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
        .load("StainedGlassLamp.gltf".to_string());
    let handle_id = handle.id();
    app.update();
    for _ in 0..LARGE_ITERATION_COUNT {
        app.update();
        match asset_server.get_load_state(handle_id).unwrap() {
            LoadState::Loaded => break,
            LoadState::Failed(err) => panic!("{err}"),
            _ => {}
        }
    }
    assert!(matches!(
        asset_server.get_load_state(handle_id).unwrap(),
        LoadState::Loaded
    ));

    // All 19 basisu textures were transcoded by the handler.
    let images = app.world().resource::<Assets<Image>>();
    assert_eq!(images.len(), 19);
    for (_, image) in images.iter() {
        assert!(image.data.as_ref().is_some_and(|data| !data.is_empty()));
    }
}
