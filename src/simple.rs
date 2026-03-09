use crate::{debug::LoadingImages, render::TerrainMaterialPlugin};
use bevy::{prelude::*, reflect::TypePath, render::render_resource::*, shader::ShaderRef};
use std::path::Path;

const DEFAULT_GRADIENT_TEXTURE: &str = "textures/gradient1.png";
const SIMPLE_TERRAIN_SHADER: &str = "shaders/spherical.wgsl";

const MODE_HEIGHT_GRADIENT: u32 = 0;
const MODE_EARTH: u32 = 1;
const MODE_ALBEDO: u32 = 2;

#[derive(ShaderType, Clone, Copy, Debug)]
pub struct SimpleTerrainStyle {
    mode: u32,
}

/// A minimal terrain material with sensible defaults for getting started quickly.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct SimpleTerrainMaterial {
    #[texture(0)]
    #[sampler(1)]
    gradient: Handle<Image>,
    #[uniform(2)]
    style: SimpleTerrainStyle,
}

impl SimpleTerrainMaterial {
    /// Color terrain from height using the built-in gradient texture.
    pub fn height_gradient(asset_server: &AssetServer, loading_images: &mut LoadingImages) -> Self {
        Self::with_mode(asset_server, loading_images, MODE_HEIGHT_GRADIENT)
    }

    /// Color terrain from its `albedo` attachment.
    pub fn albedo(asset_server: &AssetServer, loading_images: &mut LoadingImages) -> Self {
        Self::with_mode(asset_server, loading_images, MODE_ALBEDO)
    }

    /// Use albedo when the terrain folder contains an `albedo/` attachment, otherwise
    /// fall back to the built-in height gradient.
    pub fn for_terrain(
        asset_server: &AssetServer,
        loading_images: &mut LoadingImages,
        terrain_asset_path: &str,
    ) -> Self {
        if terrain_has_albedo(terrain_asset_path) {
            Self::with_mode(asset_server, loading_images, MODE_ALBEDO)
        } else {
            Self::with_mode(asset_server, loading_images, MODE_HEIGHT_GRADIENT)
        }
    }

    /// Backwards-compatible alias for [`Self::for_terrain`].
    pub fn earth_auto(
        asset_server: &AssetServer,
        loading_images: &mut LoadingImages,
        terrain_asset_path: &str,
    ) -> Self {
        Self::for_terrain(asset_server, loading_images, terrain_asset_path)
    }

    /// Use the built-in Earth shading mode from the original spherical example.
    pub fn earth(asset_server: &AssetServer, loading_images: &mut LoadingImages) -> Self {
        Self::with_mode(asset_server, loading_images, MODE_EARTH)
    }

    fn with_mode(
        asset_server: &AssetServer,
        loading_images: &mut LoadingImages,
        mode: u32,
    ) -> Self {
        let gradient = asset_server.load(DEFAULT_GRADIENT_TEXTURE);
        loading_images.load_image(
            &gradient,
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
        );

        Self {
            gradient,
            style: SimpleTerrainStyle { mode },
        }
    }
}

impl Material for SimpleTerrainMaterial {
    fn fragment_shader() -> ShaderRef {
        SIMPLE_TERRAIN_SHADER.into()
    }
}

/// Plugin that wires up [`SimpleTerrainMaterial`] and the small amount of image-finalization
/// plumbing it needs for built-in textures.
pub struct SimpleTerrainMaterialPlugin;

impl Plugin for SimpleTerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadingImages>()
            .add_plugins(TerrainMaterialPlugin::<SimpleTerrainMaterial>::default())
            .add_systems(Update, finalize_simple_material_images);
    }
}

fn finalize_simple_material_images(
    asset_server: Res<AssetServer>,
    mut loading_images: ResMut<LoadingImages>,
    mut images: ResMut<Assets<Image>>,
) {
    loading_images.finalize_ready_images(&asset_server, &mut images);
}

fn terrain_has_albedo(terrain_asset_path: &str) -> bool {
    let fs_path = if terrain_asset_path.starts_with("assets/") {
        terrain_asset_path.to_string()
    } else {
        format!("assets/{terrain_asset_path}")
    };

    Path::new(&fs_path).join("albedo").is_dir()
}
