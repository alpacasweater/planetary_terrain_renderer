use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::prelude::*;
use std::{env, path::PathBuf};

const RADIUS: f64 = 6_371_000.0;
const DEFAULT_TERRAIN_ROOT: &str = "terrains/earth";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAM_ONLINE_ENV: &str = "TERRAIN_STREAM_ONLINE";
const STREAM_HEIGHT_ENV: &str = "TERRAIN_STREAM_HEIGHT";

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
    ))
    .insert_resource(terrain_settings_from_env())
    .add_systems(Startup, setup);

    if let Some(streaming_settings) = streaming_settings_from_env() {
        app.insert_resource(streaming_settings);
    }

    app.run();
}

fn terrain_settings_from_env() -> TerrainSettings {
    let settings = TerrainSettings::with_albedo();
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

fn env_var_enabled(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
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
        asset_server.load(terrain_config),
        TerrainViewConfig::default(),
        SimpleTerrainMaterial::for_terrain(&asset_server, &mut loading_images, &terrain_root),
        view,
    );
}
