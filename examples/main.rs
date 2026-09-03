//! Loads the Khronos `StainedGlassLamp` sample model, whose 19 textures all
//! use `KHR_texture_basisu` (ETC1S and UASTC KTX2, some with zstd
//! supercompression), through the `GltfBasisuDecoderPlugin` extension
//! handler. See `assets/README.md` for the model's license.
//!
//! Run with: `cargo run --example main --release`

use bevy::{
    gltf::{GltfAssetLabel, GltfLoaderSettings},
    light::CascadeShadowConfigBuilder,
    prelude::*,
};
use bevy_gltf_basisu::GltfBasisuDecoderPlugin;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GltfBasisuDecoderPlugin))
        .add_systems(Startup, setup)
        .run();
}

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.4, 0.5, 0.9).looking_at(Vec3::new(0.0, 0.25, 0.0), Vec3::Y),
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 100.0,
            ..default()
        }
        .build(),
    ));

    // Textures that omit the standard `source` (as `KHR_texture_basisu`
    // allows) fail glTF validation, so it is disabled here.
    commands.spawn(WorldAssetRoot(
        asset_server
            .load_builder()
            .with_settings(|settings: &mut GltfLoaderSettings| settings.validate = false)
            .load(
                GltfAssetLabel::Scene(0)
                    .from_asset("models/StainedGlassLamp/StainedGlassLamp.gltf"),
            ),
    ));
}
