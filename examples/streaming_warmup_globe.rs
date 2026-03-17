use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::math::{
    geodesy::{ecef_to_lla_hae, ned_to_ecef, unit_from_lat_lon_degrees, LlaHae, Ned},
    Coordinate, TerrainShape,
};
use bevy_terrain::prelude::*;
use big_space::prelude::{FloatingOrigin, Grid};
use std::{env, f32::consts::TAU, path::PathBuf};

const DEFAULT_TERRAIN_ROOT: &str = "terrains/earth";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAMING_MAX_LOD_ENV: &str = "TERRAIN_STREAMING_MAX_LOD";
const DEFAULT_STREAMING_MAX_LOD: u32 = 6;
const WARMUP_TARGET_LAT_ENV: &str = "STREAM_WARMUP_TARGET_LAT";
const WARMUP_TARGET_LON_ENV: &str = "STREAM_WARMUP_TARGET_LON";
const WARMUP_DESCENT_SECONDS_ENV: &str = "STREAM_WARMUP_DESCENT_SECONDS";
const WARMUP_ORBIT_PERIOD_SECONDS_ENV: &str = "STREAM_WARMUP_ORBIT_PERIOD_SECONDS";
const WARMUP_EXIT_AFTER_SECONDS_ENV: &str = "STREAM_WARMUP_EXIT_AFTER_SECONDS";

const DEFAULT_TARGET_LAT_DEG: f64 = 37.705;
const DEFAULT_TARGET_LON_DEG: f64 = -122.495;
const DEFAULT_DESCENT_SECONDS: f32 = 18.0;
const DEFAULT_ORBIT_PERIOD_SECONDS: f32 = 24.0;
const START_ALTITUDE_M: f32 = 2_400_000.0;
const END_ALTITUDE_M: f32 = 28_000.0;
const START_BACKOFF_M: f32 = 1_600_000.0;
const END_BACKOFF_M: f32 = 30_000.0;
const START_SWEEP_RADIUS_M: f64 = 240_000.0;
const END_SWEEP_RADIUS_M: f64 = 18_000.0;

#[derive(Component)]
struct WarmupCamera;

#[derive(Resource, Clone)]
struct WarmupTerrainRoot(String);

#[derive(Resource)]
struct WarmupFlightPlan {
    target_origin: LlaHae,
    descent_seconds: f32,
    orbit_period_seconds: f32,
    auto_exit_after_seconds: Option<f32>,
}

#[derive(Resource, Default)]
struct WarmupRuntime {
    elapsed_seconds: f32,
    announced_close_pass: bool,
}

fn main() {
    // Load .env.opentopography.local if present (does not override already-set env vars).
    let _ = dotenvy::from_filename(".env.opentopography.local");

    let terrain_root = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_TERRAIN_ROOT.to_string());

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1600, 900),
                    title: "Streaming Warmup Globe".into(),
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
    .insert_resource(WarmupTerrainRoot(terrain_root))
    .insert_resource(terrain_settings_for_warmup())
    .insert_resource(streaming_settings_for_warmup())
    .insert_resource(warmup_flight_plan_from_env())
    .insert_resource(WarmupRuntime::default())
    .add_systems(Startup, setup)
    .add_systems(Update, run_warmup_camera);

    app.run();
}

fn terrain_settings_for_warmup() -> TerrainSettings {
    let settings = TerrainSettings::with_albedo()
        .with_streaming_cache_root(streaming_cache_root_from_env())
        .with_streaming_target_lod_count(streaming_target_lod_count_from_env());

    if !height_streaming_enabled() {
        info!(
            "OpenTopography API key not found — warmup demo will stream imagery only. \
             To include height data, add your key to .env.opentopography.local: \
             OPENTOPOGRAPHY_API_KEY=your-key-here"
        );
    }

    settings
}

fn streaming_settings_for_warmup() -> TerrainStreamingSettings {
    if height_streaming_enabled() {
        TerrainStreamingSettings::online_imagery_and_height()
    } else {
        TerrainStreamingSettings::online_imagery()
    }
}

fn streaming_cache_root_from_env() -> String {
    env::var(STREAMING_CACHE_ROOT_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "streaming_cache".to_string())
}

fn streaming_target_lod_count_from_env() -> u32 {
    env::var(STREAMING_MAX_LOD_ENV)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_STREAMING_MAX_LOD)
}

fn height_streaming_enabled() -> bool {
    env::var("OPENTOPOGRAPHY_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .is_some()
        || env::var("OPEN_TOPOGRAPHY_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
}

fn warmup_flight_plan_from_env() -> WarmupFlightPlan {
    let target_lat_deg = env_f64(WARMUP_TARGET_LAT_ENV, DEFAULT_TARGET_LAT_DEG);
    let target_lon_deg = env_f64(WARMUP_TARGET_LON_ENV, DEFAULT_TARGET_LON_DEG);
    let descent_seconds = env_f32(WARMUP_DESCENT_SECONDS_ENV, DEFAULT_DESCENT_SECONDS).max(1.0);
    let orbit_period_seconds = env_f32(
        WARMUP_ORBIT_PERIOD_SECONDS_ENV,
        DEFAULT_ORBIT_PERIOD_SECONDS,
    )
    .max(4.0);
    let auto_exit_after_seconds = env::var(WARMUP_EXIT_AFTER_SECONDS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| *value > 0.0);

    info!(
        "Streaming warmup target lat={target_lat_deg:.4}, lon={target_lon_deg:.4}, descent_s={descent_seconds:.1}, orbit_period_s={orbit_period_seconds:.1}, max_lod={}",
        streaming_target_lod_count_from_env(),
    );

    WarmupFlightPlan {
        target_origin: LlaHae {
            lat_deg: target_lat_deg,
            lon_deg: target_lon_deg,
            hae_m: 0.0,
        },
        descent_seconds,
        orbit_period_seconds,
        auto_exit_after_seconds,
    }
}

fn env_f64(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn env_f32(name: &str, default: f32) -> f32 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(default)
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut loading_images: ResMut<LoadingImages>,
    terrain_root: Res<WarmupTerrainRoot>,
) {
    let terrain_root = terrain_root.0.clone();
    let terrain_config = format!("{terrain_root}/config.tc.ron");
    let terrain_config_fs = PathBuf::from("assets").join(&terrain_config);

    if !terrain_config_fs.is_file() {
        warn!(
            "Missing terrain config at {}. Restore the repo starter assets, run `cargo run -p bevy_terrain_preprocess --example preprocess_tutorial_earth`, or pass a different terrain root as the first example argument.",
            terrain_config_fs.display()
        );
        return;
    }

    let initial_transform = camera_transform_for_focus(
        DEFAULT_TARGET_LAT_DEG,
        DEFAULT_TARGET_LON_DEG,
        START_ALTITUDE_M,
        START_BACKOFF_M,
    );

    let mut view = Entity::PLACEHOLDER;
    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                Camera3d::default(),
                FloatingOrigin,
                initial_transform,
                WarmupCamera,
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

fn run_warmup_camera(
    time: Res<Time>,
    plan: Res<WarmupFlightPlan>,
    mut runtime: ResMut<WarmupRuntime>,
    mut cameras: Query<&mut Transform, With<WarmupCamera>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    runtime.elapsed_seconds += time.delta_secs();
    let descend_t = smoothstep01(runtime.elapsed_seconds / plan.descent_seconds);
    let altitude_m = lerp_f32(START_ALTITUDE_M, END_ALTITUDE_M, descend_t);
    let backoff_m = lerp_f32(START_BACKOFF_M, END_BACKOFF_M, descend_t);
    let sweep_radius_m = lerp_f64(START_SWEEP_RADIUS_M, END_SWEEP_RADIUS_M, descend_t as f64);

    let angular_phase = TAU * (runtime.elapsed_seconds / plan.orbit_period_seconds);
    let focus_lla = swept_focus_lla(&plan.target_origin, sweep_radius_m, angular_phase);
    let transform =
        camera_transform_for_focus(focus_lla.lat_deg, focus_lla.lon_deg, altitude_m, backoff_m);

    for mut camera_transform in &mut cameras {
        *camera_transform = transform;
    }

    if descend_t >= 0.999 && !runtime.announced_close_pass {
        runtime.announced_close_pass = true;
        info!(
            "Warmup demo reached close pass over lat={:.4}, lon={:.4}. Cache fill should now be visible in assets/{}/",
            focus_lla.lat_deg,
            focus_lla.lon_deg,
            streaming_cache_root_from_env(),
        );
    }

    if let Some(exit_after_seconds) = plan.auto_exit_after_seconds {
        if runtime.elapsed_seconds >= exit_after_seconds {
            info!(
                "Warmup demo reached STREAM_WARMUP_EXIT_AFTER_SECONDS={exit_after_seconds:.1}. Exiting."
            );
            app_exit.write(AppExit::Success);
        }
    }
}

fn swept_focus_lla(origin: &LlaHae, sweep_radius_m: f64, angular_phase: f32) -> LlaHae {
    let offset = Ned {
        n_m: sweep_radius_m * angular_phase.cos() as f64,
        e_m: sweep_radius_m * angular_phase.sin() as f64,
        d_m: 0.0,
    };
    ecef_to_lla_hae(ned_to_ecef(offset, *origin))
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
    let target = Coordinate::from_lat_lon_degrees(lat_deg, lon_deg)
        .local_position(TerrainShape::WGS84, 0.0)
        .as_vec3();

    let mut east = Vec3::Y.cross(up);
    if east.length_squared() < 1e-6 {
        east = Vec3::Z.cross(up);
    }
    east = east.normalize();
    let north = up.cross(east).normalize();

    let camera_position = target + up * altitude_m + north * backoff_m + east * (0.2 * backoff_m);
    Transform::from_translation(camera_position).looking_at(target, up)
}

fn smoothstep01(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_f32(start: f32, end: f32, t: f32) -> f32 {
    start + (end - start) * t
}

fn lerp_f64(start: f64, end: f64, t: f64) -> f64 {
    start + (end - start) * t
}
