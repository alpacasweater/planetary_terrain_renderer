use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::prelude::*;
use std::{env, path::Path};

const RADIUS: f64 = 6_371_000.0;
const EARTH_CONFIG_PATH: &str = "assets/terrains/earth/config.tc.ron";
const EARTH_ASSET_PATH: &str = "terrains/earth";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAM_ONLINE_ENV: &str = "TERRAIN_STREAM_ONLINE";
const STREAM_HEIGHT_ENV: &str = "TERRAIN_STREAM_HEIGHT";
const STREAMING_MAX_LOD_ENV: &str = "TERRAIN_STREAMING_MAX_LOD";
const DEFAULT_STREAMING_MAX_LOD: u32 = 6;

fn main() {
    // Load .env.opentopography.local if present (does not override already-set env vars).
    let _ = dotenvy::from_filename(".env.opentopography.local");

    let mut app = App::new();
    app.add_plugins((
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
        SimpleTerrainMaterialPlugin,
        TerrainDebugPlugin,
        TerrainPickingPlugin,
    ))
    .insert_resource(terrain_settings_from_env())
    .add_systems(Startup, initialize);

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

    let height_disabled_explicitly = matches!(
        env::var(STREAM_HEIGHT_ENV).ok().as_deref(),
        Some("0") | Some("false") | Some("FALSE") | Some("no") | Some("NO")
    );
    let stream_height = env_var_enabled(STREAM_HEIGHT_ENV)
        || (height_api_key_present() && !height_disabled_explicitly);

    Some(if stream_height {
        TerrainStreamingSettings::online_imagery_and_height()
    } else {
        TerrainStreamingSettings::online_imagery()
    })
}

fn streaming_requested() -> bool {
    // Also treat API key presence as implicit request for height (and thus online) streaming.
    env_var_enabled(STREAM_ONLINE_ENV)
        || env_var_enabled(STREAM_HEIGHT_ENV)
        || height_api_key_present()
}

fn height_api_key_present() -> bool {
    env::var("OPENTOPOGRAPHY_API_KEY")
        .or_else(|_| env::var("OPEN_TOPOGRAPHY_API_KEY"))
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
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

fn initialize(
    mut commands: Commands,
    mut loading_images: ResMut<LoadingImages>,
    asset_server: Res<AssetServer>,
) {
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

    if !Path::new(EARTH_CONFIG_PATH).is_file() {
        warn!(
            "Missing Earth terrain at {EARTH_CONFIG_PATH}. Restore the repo starter assets or run `./scripts/setup_earth_quickstart.sh`."
        );
        return;
    }

    commands.spawn_terrain(
        asset_server.load("terrains/earth/config.tc.ron"),
        TerrainViewConfig::default(),
        SimpleTerrainMaterial::for_terrain(&asset_server, &mut loading_images, EARTH_ASSET_PATH),
        view,
    );
}
