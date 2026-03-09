use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::prelude::*;
use std::{env, path::Path};

const RADIUS: f64 = 6_371_000.0;
const EARTH_CONFIG_PATH: &str = "assets/terrains/earth/config.tc.ron";
const EARTH_ASSET_PATH: &str = "terrains/earth";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAM_ONLINE_ENV: &str = "TERRAIN_STREAM_ONLINE";

fn main() {
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

    if env_var_enabled(STREAM_ONLINE_ENV) {
        app.insert_resource(TerrainStreamingSettings::online_imagery());
    }

    app.run();
}

fn terrain_settings_from_env() -> TerrainSettings {
    let settings = TerrainSettings::with_albedo();
    match env::var(STREAMING_CACHE_ROOT_ENV) {
        Ok(root) if !root.trim().is_empty() => settings.with_streaming_cache_root(root),
        _ => settings,
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
