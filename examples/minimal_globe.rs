use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::{math::geodesy::unit_from_lat_lon_degrees, prelude::*};
use std::{env, path::PathBuf};

const RADIUS: f64 = 6_371_000.0;
const DEFAULT_TERRAIN_ROOT: &str = "terrains/earth";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAM_ONLINE_ENV: &str = "TERRAIN_STREAM_ONLINE";
const STREAM_HEIGHT_ENV: &str = "TERRAIN_STREAM_HEIGHT";
const STREAMING_MAX_LOD_ENV: &str = "TERRAIN_STREAMING_MAX_LOD";
const DEFAULT_STREAMING_MAX_LOD: u32 = 10;
const CAMERA_TARGET_LAT_ENV: &str = "MINIMAL_GLOBE_TARGET_LAT";
const CAMERA_TARGET_LON_ENV: &str = "MINIMAL_GLOBE_TARGET_LON";
const CAMERA_ALTITUDE_ENV: &str = "MINIMAL_GLOBE_CAMERA_ALTITUDE_M";
const CAMERA_BACKOFF_ENV: &str = "MINIMAL_GLOBE_CAMERA_BACKOFF_M";
const DEFAULT_CAMERA_ALTITUDE_M: f32 = 120_000.0;
const DEFAULT_CAMERA_BACKOFF_M: f32 = 80_000.0;

fn main() {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1600, 900),
                    title: "Minimal Globe".into(),
                    ..default()
                }),
                ..default()
            })
            .build()
            .disable::<TransformPlugin>(),
        TerrainPlugin,
        SimpleTerrainMaterialPlugin,
        TerrainDebugPlugin,
        TerrainPickingPlugin,
    ))
    .insert_resource(terrain_settings_from_env())
    .add_systems(Startup, setup);

    if let Some(streaming_settings) = streaming_settings_from_env() {
        app.insert_resource(streaming_settings);
    }

    app.run();
}

fn terrain_settings_from_env() -> TerrainSettings {
    let mut settings = TerrainSettings::with_albedo();
    if let Some(max_lod) = streaming_target_lod_count_from_env() {
        settings = settings.with_streaming_target_lod_count(max_lod);
    }

    match env::var(STREAMING_CACHE_ROOT_ENV) {
        Ok(root) if !root.trim().is_empty() => settings.with_streaming_cache_root(root),
        _ if streaming_requested() => settings.with_streaming_cache_root("streaming_cache"),
        _ => settings,
    }
}

fn streaming_settings_from_env() -> Option<TerrainStreamingSettings> {
    if !streaming_requested() {
        return None;
    }

    Some(if env_var_enabled(STREAM_HEIGHT_ENV) {
        TerrainStreamingSettings::online_imagery_and_height()
    } else {
        TerrainStreamingSettings::online_imagery()
    })
}

fn streaming_requested() -> bool {
    env_var_enabled(STREAM_ONLINE_ENV) || env_var_enabled(STREAM_HEIGHT_ENV)
}

fn streaming_target_lod_count_from_env() -> Option<u32> {
    match env::var(STREAMING_MAX_LOD_ENV) {
        Ok(value) => value.trim().parse().ok(),
        Err(_) if streaming_requested() => Some(DEFAULT_STREAMING_MAX_LOD),
        Err(_) => None,
    }
}

fn env_var_enabled(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn env_f32(name: &str, default: f32) -> f32 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str) -> Option<f64> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
}

fn camera_transform_for_overview() -> Transform {
    Transform::from_translation(-Vec3::X * RADIUS as f32 * 3.0).looking_to(Vec3::X, Vec3::Y)
}

fn camera_transform_for_focus(
    lat_deg: f64,
    lon_deg: f64,
    altitude_m: f32,
    backoff_m: f32,
) -> Transform {
    let up = unit_from_lat_lon_degrees(lat_deg, lon_deg)
        .as_vec3()
        .normalize();
    let target = up * RADIUS as f32;

    let mut east = Vec3::Y.cross(up);
    if east.length_squared() < 1e-6 {
        east = Vec3::Z.cross(up);
    }
    east = east.normalize();
    let north = up.cross(east).normalize();

    let camera_position = target + up * altitude_m + north * backoff_m;
    Transform::from_translation(camera_position).looking_at(target, north)
}

fn initial_camera_transform_from_env() -> Transform {
    match (
        env_f64(CAMERA_TARGET_LAT_ENV),
        env_f64(CAMERA_TARGET_LON_ENV),
    ) {
        (Some(lat_deg), Some(lon_deg)) => camera_transform_for_focus(
            lat_deg,
            lon_deg,
            env_f32(CAMERA_ALTITUDE_ENV, DEFAULT_CAMERA_ALTITUDE_M),
            env_f32(CAMERA_BACKOFF_ENV, DEFAULT_CAMERA_BACKOFF_M),
        ),
        _ => camera_transform_for_overview(),
    }
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut loading_images: ResMut<LoadingImages>,
) {
    // The first CLI argument optionally overrides the terrain root inside `assets/`.
    let terrain_root = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_TERRAIN_ROOT.to_string());
    let terrain_config = format!("{terrain_root}/config.tc.ron");
    let terrain_config_fs = PathBuf::from("assets").join(&terrain_config);

    if !terrain_config_fs.is_file() {
        warn!(
            "Missing terrain config at {}. Restore the repo starter assets, run `cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth`, or pass a different terrain root as the first example argument.",
            terrain_config_fs.display()
        );
        return;
    }

    info!(
        "Controls: scroll or right-drag to zoom, left-drag to pan, middle-drag to orbit, T toggles fly camera."
    );

    if let Ok(config) = TerrainConfig::load_file(&terrain_config_fs) {
        if terrain_root == DEFAULT_TERRAIN_ROOT && config.lod_count < 5 {
            warn!(
                "Bundled Earth is a coarse starter dataset (lod_count={}). Steep relief like the Alps will look soft unless you use a higher-resolution terrain root, cached higher-LOD tiles, or TERRAIN_STREAM_HEIGHT=1 with OpenTopography.",
                config.lod_count
            );
        }
    }

    let mut view = Entity::PLACEHOLDER;
    let initial_transform = initial_camera_transform_from_env();

    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                initial_transform,
                DebugCameraController::new(RADIUS),
                OrbitalCameraController::default(),
            ))
            .id();
    });

    commands.spawn_terrain(
        asset_server.load(terrain_config),
        TerrainViewConfig::default(),
        SimpleTerrainMaterial::for_terrain(&asset_server, &mut loading_images, &terrain_root),
        view,
    );
}
