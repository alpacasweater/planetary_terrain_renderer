use bevy::window::WindowResolution;
use bevy::{prelude::*, reflect::TypePath, render::render_resource::*};
use bevy::shader::ShaderRef;
use bevy_terrain::prelude::*;
use std::path::Path;
use std::{collections::HashMap, env};

const RADIUS: f64 = 6371000.0;
const BASE_TERRAIN_CONFIG: &str = "terrains/earth/config.tc.ron";
const DEFAULT_OVERLAY_KEYS: &[&str] = &["swiss"];
const OVERLAY_ENV: &str = "MULTIRES_OVERLAYS";

#[derive(ShaderType, Clone)]
struct GradientInfo {
    mode: u32,
}

#[derive(Asset, AsBindGroup, TypePath, Clone)]
pub struct CustomMaterial {
    #[texture(0)]
    #[sampler(1)]
    gradient: Handle<Image>,
    #[uniform(2)]
    gradient_info: GradientInfo,
}

impl Material for CustomMaterial {
    fn fragment_shader() -> ShaderRef {
        "shaders/spherical.wgsl".into()
    }
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        resolution: WindowResolution::new(1920, 1080),
                        ..default()
                    }),
                    ..default()
                })
                .build()
                .disable::<TransformPlugin>(),
            TerrainPlugin,
            TerrainMaterialPlugin::<CustomMaterial>::default(),
            TerrainDebugPlugin,
            TerrainPickingPlugin,
        ))
        .insert_resource(TerrainSettings::new(vec!["albedo"]))
        .add_systems(Startup, initialize)
        .run();
}

fn asset_exists(asset_path: &str) -> bool {
    let fs_path = format!("assets/{asset_path}");
    Path::new(&fs_path).is_file()
}

fn overlay_config_map() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("swiss", "terrains/swiss_highres/config.tc.ron"),
        ("saxony", "terrains/saxony_partial/config.tc.ron"),
        ("los", "terrains/los_highres/config.tc.ron"),
        ("srtm_n27e086", "terrains/srtm_n27e086/config.tc.ron"),
        ("srtm_n35e139", "terrains/srtm_n35e139/config.tc.ron"),
        ("srtm_n37e127", "terrains/srtm_n37e127/config.tc.ron"),
        ("srtm_n39w077", "terrains/srtm_n39w077/config.tc.ron"),
        ("srtm_n51e000", "terrains/srtm_n51e000/config.tc.ron"),
        ("srtm_s22w043", "terrains/srtm_s22w043/config.tc.ron"),
        ("srtm_s33e151", "terrains/srtm_s33e151/config.tc.ron"),
    ])
}

fn selected_overlay_keys() -> Vec<String> {
    match env::var(OVERLAY_ENV) {
        Ok(value) => {
            if value.trim().is_empty() || value.trim().eq_ignore_ascii_case("none") {
                return Vec::new();
            }
            let mut seen = std::collections::HashSet::new();
            let mut keys = Vec::new();
            for part in value.split(',') {
                let key = part.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
            keys
        }
        Err(_) => DEFAULT_OVERLAY_KEYS.iter().map(|s| s.to_string()).collect(),
    }
}

#[allow(clippy::too_many_arguments)]
fn initialize(
    mut commands: Commands,
    mut images: ResMut<LoadingImages>,
    asset_server: Res<AssetServer>,
) {
    let overlay_map = overlay_config_map();
    let selected_keys = selected_overlay_keys();

    let gradient = asset_server.load("textures/gradient1.png");
    images.load_image(
        &gradient,
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
    );

    let mut view = Entity::PLACEHOLDER;
    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                Transform::from_translation(-Vec3::X * RADIUS as f32 * 3.0)
                    .looking_to(Vec3::X, Vec3::Y),
                DebugCameraController::new(RADIUS),
                OrbitalCameraController::default(),
            ))
            .id();
    });

    commands.spawn_terrain(
        asset_server.load(BASE_TERRAIN_CONFIG),
        TerrainViewConfig::default(),
        CustomMaterial {
            gradient: gradient.clone(),
            gradient_info: GradientInfo { mode: 2 },
        },
        view,
    );

    let mut loaded_overlays = 0_u32;
    for key in &selected_keys {
        let Some(&config_path) = overlay_map.get(key.as_str()) else {
            warn!("Unknown overlay key '{key}', skipping.");
            continue;
        };

        if !asset_exists(config_path) {
            warn!("Overlay config missing at '{config_path}', skipping.");
            continue;
        }

        commands.spawn_terrain(
            asset_server.load(config_path),
            TerrainViewConfig {
                order: 1,
                ..default()
            },
            CustomMaterial {
                gradient: gradient.clone(),
                gradient_info: GradientInfo { mode: 0 },
            },
            view,
        );
        loaded_overlays += 1;
    }

    info!("Loaded base terrain: {BASE_TERRAIN_CONFIG}");
    info!("Overlay selection from {OVERLAY_ENV}: {:?}", selected_keys);
    info!("Loaded overlays: {loaded_overlays}");
}
