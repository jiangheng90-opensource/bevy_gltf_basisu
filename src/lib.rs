//! Bevy plugin that adds `KHR_texture_basisu` support to the glTF loader.
//!
//! The plugin registers a [`GltfExtensionHandler`] that claims textures
//! carrying the `KHR_texture_basisu` extension and transcodes their KTX2
//! Basis Universal (ETC1S / UASTC) payloads with the official
//! `basis_transcoder`, choosing the best GPU compressed format available.
//!
//! Requires bevy with the `on_texture_load` glTF extension hook
//! (<https://github.com/bevyengine/bevy/pull/25669>). Textures that omit the
//! standard `source` (allowed by `KHR_texture_basisu`) fail glTF validation,
//! so load glTFs with `GltfLoaderSettings { validate: false, .. }`.
//!
//! # Example
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_gltf_basisu::GltfBasisuDecoderPlugin;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(GltfBasisuDecoderPlugin)
//!         .run();
//! }
//! ```

use basis_transcoder::{TargetFormat, TranscodedTexture};
use bevy::asset::{AssetPath, LoadContext, RenderAssetUsages};
use bevy::gltf::extensions::{ErasedGltfExtensionHandler, GltfExtensionHandler};
use bevy::gltf::gltf::{self, Gltf as JsonGltf, Texture};
use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageSamplerDescriptor};
use bevy::{
    app::{App, Plugin},
    gltf::extensions::GltfExtensionHandlers,
    log::error,
};
use wgpu_types::{
    AstcBlock, AstcChannel, Extent3d, TextureDataOrder, TextureDimension, TextureFormat,
};

/// 12-byte KTX2 file identifier.
const KTX2_MAGIC: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];

/// Returns true if the bytes start with the KTX2 file identifier.
fn is_ktx2(buffer: &[u8]) -> bool {
    buffer.starts_with(&KTX2_MAGIC)
}

/// Picks the best transcode target for the GPU's compressed-format support,
/// falling back to uncompressed RGBA8 ([`CompressedImageFormats::NONE`]).
///
/// On WASM the reported formats may be empty (no GPU capability information
/// at load time, e.g. when the render device is not initialized yet), in
/// which case the RGBA8 fallback applies.
fn select_target_format(formats: CompressedImageFormats) -> TargetFormat {
    if formats.contains(CompressedImageFormats::BC) {
        TargetFormat::Bc7Rgba
    } else if formats.contains(CompressedImageFormats::ETC2) {
        TargetFormat::Etc2Rgba
    } else if formats.contains(CompressedImageFormats::ASTC_LDR) {
        TargetFormat::Astc4x4Rgba
    } else {
        TargetFormat::Rgba32
    }
}

/// Maps the transcode target to the wgpu texture format; `is_srgb` selects the
/// sRGB variant (the basisu target formats are color-space agnostic).
fn wgpu_format(target: TargetFormat, is_srgb: bool) -> Option<TextureFormat> {
    Some(match target {
        TargetFormat::Bc7Rgba => {
            if is_srgb {
                TextureFormat::Bc7RgbaUnormSrgb
            } else {
                TextureFormat::Bc7RgbaUnorm
            }
        }
        TargetFormat::Etc2Rgba => {
            if is_srgb {
                TextureFormat::Etc2Rgba8UnormSrgb
            } else {
                TextureFormat::Etc2Rgba8Unorm
            }
        }
        TargetFormat::Astc4x4Rgba => TextureFormat::Astc {
            block: AstcBlock::B4x4,
            channel: if is_srgb {
                AstcChannel::UnormSrgb
            } else {
                AstcChannel::Unorm
            },
        },
        TargetFormat::Rgba32 => {
            if is_srgb {
                TextureFormat::Rgba8UnormSrgb
            } else {
                TextureFormat::Rgba8Unorm
            }
        }
        _ => return None,
    })
}

/// Assembles a bevy [`Image`] from a transcoded texture, packing every mip
/// level into one buffer in mip-major order.
fn build_image(
    texture: TranscodedTexture,
    is_srgb: bool,
    sampler: ImageSampler,
    render_asset_usages: RenderAssetUsages,
) -> Result<Image, String> {
    let format = wgpu_format(texture.format, is_srgb)
        .ok_or_else(|| format!("unsupported transcoded format: {:?}", texture.format))?;

    let mut data = Vec::with_capacity(texture.levels.iter().map(|l| l.data.len()).sum());
    for level in &texture.levels {
        data.extend_from_slice(&level.data);
    }

    let mut out = Image {
        data: Some(data),
        data_order: TextureDataOrder::MipMajor,
        sampler,
        ..Default::default()
    };
    out.texture_descriptor.size = Extent3d {
        width: texture.info.width,
        height: texture.info.height,
        depth_or_array_layers: 1,
    };
    out.texture_descriptor.mip_level_count = texture.info.levels;
    out.texture_descriptor.format = format;
    out.texture_descriptor.dimension = TextureDimension::D2;
    out.asset_usage = render_asset_usages;
    Ok(out)
}

/// Transcodes a KTX2 Basis Universal texture into a bevy [`Image`], choosing
/// the best target format for the GPU's compressed-format support
/// ([`CompressedImageFormats::NONE`] yields uncompressed RGBA8).
async fn transcode_ktx2_image(
    buffer: &[u8],
    supported_compressed_formats: CompressedImageFormats,
    is_srgb: bool,
    sampler: ImageSampler,
    render_asset_usages: RenderAssetUsages,
) -> Result<Image, String> {
    let target = select_target_format(supported_compressed_formats);
    let texture = basis_transcoder::transcode_ktx2(buffer, target)
        .await
        .ok_or_else(|| "basisu transcode failed".to_string())?;
    build_image(texture, is_srgb, sampler, render_asset_usages)
}

/// Decodes a `data:` URI into its bytes.
fn decode_data_uri(uri: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let (_, data) = uri.split_once(',')?;
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// Internal handler that loads `KHR_texture_basisu` textures for each glTF
/// texture that declares the extension.
#[derive(Default, Clone)]
struct GltfBasisuDecoderExtensionHandler;

impl GltfBasisuDecoderExtensionHandler {
    /// Resolves the bytes of the image referenced by the
    /// `KHR_texture_basisu` extension of the given texture.
    async fn image_bytes(
        load_context: &mut LoadContext<'_>,
        gltf: &JsonGltf,
        gltf_texture: &Texture<'_>,
        buffer_data: &[Vec<u8>],
        gltf_path: &AssetPath<'_>,
    ) -> Option<Vec<u8>> {
        // `KHR_texture_basisu` moves the image reference into the extension
        // and may omit the standard `texture.source`, so resolve the image
        // from the extension instead.
        let index = gltf_texture
            .extension_value("KHR_texture_basisu")?
            .get("source")?
            .as_u64()? as usize;
        let image = gltf.images().nth(index)?;
        match image.source() {
            gltf::image::Source::View { view, .. } => {
                let start = view.offset();
                let end = view.offset() + view.length();
                Some(buffer_data.get(view.buffer().index())?[start..end].to_vec())
            }
            gltf::image::Source::Uri { uri, .. } => {
                let uri = percent_encoding::percent_decode_str(uri)
                    .decode_utf8()
                    .ok()?;
                if uri.starts_with("data:") {
                    decode_data_uri(&uri)
                } else {
                    let path = gltf_path.resolve_embed_str(&uri).ok()?;
                    load_context.read_asset_bytes(path).await.ok()
                }
            }
        }
    }
}

impl GltfExtensionHandler for GltfBasisuDecoderExtensionHandler {
    fn dyn_clone(&self) -> Box<dyn ErasedGltfExtensionHandler> {
        Box::new(self.clone())
    }

    async fn on_texture_load(
        &mut self,
        load_context: &mut LoadContext<'_>,
        gltf_document: &JsonGltf,
        gltf_texture: &Texture<'_>,
        buffer_data: &[Vec<u8>],
        gltf_path: &AssetPath<'_>,
        is_srgb: bool,
        sampler: ImageSamplerDescriptor,
        supported_compressed_formats: CompressedImageFormats,
        render_asset_usages: RenderAssetUsages,
        user_image: &mut Option<Image>,
    ) {
        if gltf_texture.extension_value("KHR_texture_basisu").is_none() {
            // This texture does not use KHR_texture_basisu; let the default
            // loader handle it.
            return;
        }
        let Some(bytes) = Self::image_bytes(
            load_context,
            gltf_document,
            gltf_texture,
            buffer_data,
            gltf_path,
        )
        .await
        else {
            error!(
                "failed to resolve KHR_texture_basisu image (texture {})",
                gltf_texture.index()
            );
            return;
        };
        if !is_ktx2(&bytes) {
            error!(
                "KHR_texture_basisu image is not a KTX2 file (texture {})",
                gltf_texture.index()
            );
            return;
        }
        match transcode_ktx2_image(
            &bytes,
            supported_compressed_formats,
            is_srgb,
            ImageSampler::Descriptor(sampler),
            render_asset_usages,
        )
        .await
        {
            Ok(image) => *user_image = Some(image),
            Err(err) => error!(
                "failed to transcode KHR_texture_basisu texture {}: {err}",
                gltf_texture.index()
            ),
        }
    }
}

/// Bevy plugin that adds `KHR_texture_basisu` support to the glTF loader.
///
/// Add this plugin to your app to load glTF models whose textures use the
/// `KHR_texture_basisu` extension (KTX2 Basis Universal, ETC1S / UASTC).
pub struct GltfBasisuDecoderPlugin;

impl Plugin for GltfBasisuDecoderPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(target_family = "wasm")]
        bevy::tasks::block_on(async {
            app.world_mut()
                .resource_mut::<GltfExtensionHandlers>()
                .0
                .write()
                .await
                .push(Box::new(GltfBasisuDecoderExtensionHandler));
        });
        #[cfg(not(target_family = "wasm"))]
        app.world_mut()
            .resource_mut::<GltfExtensionHandlers>()
            .0
            .write_blocking()
            .push(Box::new(GltfBasisuDecoderExtensionHandler));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn select_target_format_prefers_compressed_then_rgba8() {
        assert!(matches!(
            select_target_format(CompressedImageFormats::BC),
            TargetFormat::Bc7Rgba
        ));
        assert!(matches!(
            select_target_format(CompressedImageFormats::ETC2),
            TargetFormat::Etc2Rgba
        ));
        assert!(matches!(
            select_target_format(CompressedImageFormats::ASTC_LDR),
            TargetFormat::Astc4x4Rgba
        ));
        // No capabilities reported (e.g. wasm without GPU info at load time)
        // falls back to uncompressed RGBA8.
        assert!(matches!(
            select_target_format(CompressedImageFormats::NONE),
            TargetFormat::Rgba32
        ));
    }
}
