//! Integration test with real-world KTX2 Basis Universal assets:
//! the hardware textures of the Khronos `StainedGlassLamp` sample model
//! (CC-BY 4.0, Wayfair — see `assets/README.md`). The base color texture is
//! ETC1S (sRGB) and the normal texture is UASTC (linear).
//!
//! The glTF references the textures through external URIs and a data URI,
//! exercising the non-embedded source resolution of the handler.

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

const BASECOLOR: &[u8] = include_bytes!("../assets/StainedGlassLamp_hardware_basecolor.ktx2");
const NORMAL: &[u8] = include_bytes!("../assets/StainedGlassLamp_hardware_normal.ktx2");

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

fn expected_mip_data_len(image: &Image) -> usize {
    let desc = &image.texture_descriptor;
    let mut expected = 0;
    for level in 0..desc.mip_level_count {
        expected += (desc.size.width as usize >> level).max(1)
            * (desc.size.height as usize >> level).max(1)
            * 4;
    }
    expected
}

fn assert_transcoded(image: &Image, expected_format: TextureFormat) {
    let desc = &image.texture_descriptor;
    assert_eq!(desc.format, expected_format);
    let data = image.data.as_ref().unwrap();
    assert_eq!(data.len(), expected_mip_data_len(image));
    // Level 0 must not decode to a solid color.
    let level0_len = desc.size.width as usize * desc.size.height as usize * 4;
    let first = &data[0..4];
    assert!(
        data[..level0_len]
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p != first)
    );
}

#[test]
fn load_gltf_with_real_basisu_assets() {
    let basecolor_base64 = base64::engine::general_purpose::STANDARD.encode(BASECOLOR);
    let gltf_json = format!(
        r#"{{
    "asset": {{ "version": "2.0" }},
    "extensionsUsed": ["KHR_texture_basisu"],
    "extensionsRequired": ["KHR_texture_basisu"],
    "scene": 0,
    "scenes": [{{ "nodes": [0] }}],
    "nodes": [{{ "name": "root" }}],
    "images": [
        {{ "uri": "StainedGlassLamp_hardware_basecolor.ktx2", "mimeType": "image/ktx2" }},
        {{ "uri": "StainedGlassLamp_hardware_normal.ktx2", "mimeType": "image/ktx2" }},
        {{ "uri": "data:image/ktx2;base64,{}", "mimeType": "image/ktx2" }}
    ],
    "textures": [
        {{ "extensions": {{ "KHR_texture_basisu": {{ "source": 0 }} }} }},
        {{ "extensions": {{ "KHR_texture_basisu": {{ "source": 1 }} }} }},
        {{ "extensions": {{ "KHR_texture_basisu": {{ "source": 2 }} }} }}
    ],
    "materials": [
        {{
            "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }},
            "normalTexture": {{ "index": 1 }}
        }},
        {{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 2 }} }} }}
    ]
}}"#,
        basecolor_base64,
    );

    let gltf_path = "test.gltf";
    let dir = Dir::default();
    dir.insert_asset_text(Path::new(gltf_path), &gltf_json);
    dir.insert_asset(
        Path::new("StainedGlassLamp_hardware_basecolor.ktx2"),
        BASECOLOR.to_vec(),
    );
    dir.insert_asset(
        Path::new("StainedGlassLamp_hardware_normal.ktx2"),
        NORMAL.to_vec(),
    );

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

    // Without render device capabilities the handler transcodes to RGBA8.
    // The base color texture is sRGB, the normal texture is linear.
    let base_color = images
        .get(material.base_color_texture.as_ref().unwrap())
        .unwrap();
    assert_transcoded(base_color, TextureFormat::Rgba8UnormSrgb);

    let normal = images
        .get(material.normal_map_texture.as_ref().unwrap())
        .unwrap();
    assert_transcoded(normal, TextureFormat::Rgba8Unorm);

    // The data-URI texture is sRGB as well.
    let data_uri_material = materials.get(&gltf_root.materials[1]).unwrap();
    let data_uri_texture = images
        .get(data_uri_material.base_color_texture.as_ref().unwrap())
        .unwrap();
    assert_transcoded(data_uri_texture, TextureFormat::Rgba8UnormSrgb);
}
