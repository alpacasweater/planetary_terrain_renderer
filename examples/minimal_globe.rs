use bevy::prelude::*;
use bevy::window::WindowResolution;
use bevy_terrain::{
    math::geodesy::unit_from_lat_lon_degrees, prelude::*, streaming::NasaGibsImageryConfig,
};
use std::{env, path::PathBuf, process};

const RADIUS: f64 = 6_371_000.0;
const DEFAULT_TERRAIN_ROOT: &str = "terrains/earth";
const MAX_LOD_ENV: &str = "MINIMAL_GLOBE_MAX_LOD";
const IMAGERY_PRESET_ENV: &str = "TERRAIN_STREAM_IMAGERY_PRESET";
const STREAMING_CACHE_ROOT_ENV: &str = "TERRAIN_STREAMING_CACHE_ROOT";
const STREAM_ONLINE_ENV: &str = "TERRAIN_STREAM_ONLINE";
const STREAM_HEIGHT_ENV: &str = "TERRAIN_STREAM_HEIGHT";
const STREAMING_MAX_LOD_ENV: &str = "TERRAIN_STREAMING_MAX_LOD";
const DEFAULT_MAX_LOD: u32 = 7;
const CAMERA_TARGET_LAT_ENV: &str = "MINIMAL_GLOBE_TARGET_LAT";
const CAMERA_TARGET_LON_ENV: &str = "MINIMAL_GLOBE_TARGET_LON";
const CAMERA_ALTITUDE_ENV: &str = "MINIMAL_GLOBE_CAMERA_ALTITUDE_M";
const CAMERA_BACKOFF_ENV: &str = "MINIMAL_GLOBE_CAMERA_BACKOFF_M";
const DEFAULT_CAMERA_ALTITUDE_M: f32 = 40_000.0;
const DEFAULT_CAMERA_BACKOFF_M: f32 = 18_000.0;
const DEFAULT_HEIGHT_STREAM_MAX_INFLIGHT: usize = 2;

#[derive(Resource, Clone, Debug)]
struct MinimalGlobeOptions {
    terrain_root: String,
    max_lod: u32,
    stream_online: bool,
    stream_height: bool,
}

fn main() {
    let options = MinimalGlobeOptions::from_env_and_args();
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
    .insert_resource(options.clone())
    .insert_resource(terrain_settings_from_options(&options))
    .insert_resource(imagery_provider_for_minimal())
    .add_systems(Startup, setup);

    if let Some(streaming_settings) = streaming_settings_from_options(&options) {
        app.insert_resource(streaming_settings);
    }

    app.run();
}

impl MinimalGlobeOptions {
    fn from_env_and_args() -> Self {
        let mut terrain_root = None;
        let mut max_lod = env_u32(MAX_LOD_ENV)
            .or_else(|| env_u32(STREAMING_MAX_LOD_ENV))
            .unwrap_or(DEFAULT_MAX_LOD);
        let mut stream_online = env_var_enabled(STREAM_ONLINE_ENV);
        let mut stream_height = env_var_enabled(STREAM_HEIGHT_ENV);

        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => print_usage_and_exit(0),
                "--terrain-root" => terrain_root = Some(next_arg_value(&arg, &mut args)),
                "--max-lod" => {
                    let value = next_arg_value(&arg, &mut args);
                    max_lod = parse_u32_flag("--max-lod", &value);
                }
                "--stream-online" => stream_online = true,
                "--stream-height" => {
                    stream_online = true;
                    stream_height = true;
                }
                _ => {
                    if let Some(value) = arg.strip_prefix("--terrain-root=") {
                        terrain_root = Some(value.to_string());
                    } else if let Some(value) = arg.strip_prefix("--max-lod=") {
                        max_lod = parse_u32_flag("--max-lod", value);
                    } else if arg.starts_with("--") {
                        eprintln!("Unknown flag: {arg}");
                        print_usage_and_exit(2);
                    } else if terrain_root.is_none() {
                        terrain_root = Some(arg);
                    } else {
                        eprintln!("Unexpected extra argument: {arg}");
                        print_usage_and_exit(2);
                    }
                }
            }
        }

        Self {
            terrain_root: terrain_root.unwrap_or_else(|| DEFAULT_TERRAIN_ROOT.to_string()),
            max_lod,
            stream_online,
            stream_height,
        }
    }
}

fn print_usage_and_exit(code: i32) -> ! {
    let usage = format!(
        "Usage: cargo run --example minimal_globe -- [terrain_root] [--max-lod N] [--stream-online] [--stream-height]\n\
         \n\
         Environment overrides:\n\
         - {MAX_LOD_ENV}=N\n\
         - {STREAMING_CACHE_ROOT_ENV}=streaming_cache\n\
         - {STREAM_ONLINE_ENV}=1\n\
         - {STREAM_HEIGHT_ENV}=1\n\
         - {IMAGERY_PRESET_ENV}=eox_s2cloudless_2017|gibs_modis\n\
         - {CAMERA_TARGET_LAT_ENV}=46.55\n\
         - {CAMERA_TARGET_LON_ENV}=10.60\n\
         - {CAMERA_ALTITUDE_ENV}=120000\n\
         - {CAMERA_BACKOFF_ENV}=80000\n\
         \n\
         Height streaming examples:\n\
         - POSIX shells: OPENTOPOGRAPHY_API_KEY=your-key cargo run --example minimal_globe -- --max-lod 7 --stream-height\n\
         - PowerShell: $env:OPENTOPOGRAPHY_API_KEY=\"your-key\"; cargo run --example minimal_globe -- --max-lod 7 --stream-height\n\
         - cmd.exe: set OPENTOPOGRAPHY_API_KEY=your-key && cargo run --example minimal_globe -- --max-lod 7 --stream-height\n"
    );
    if code == 0 {
        println!("{usage}");
    } else {
        eprintln!("{usage}");
    }
    process::exit(code);
}

fn next_arg_value(flag: &str, args: &mut impl Iterator<Item = String>) -> String {
    match args.next() {
        Some(value) => value,
        None => {
            eprintln!("Missing value for {flag}");
            print_usage_and_exit(2);
        }
    }
}

fn parse_u32_flag(flag: &str, value: &str) -> u32 {
    match value.trim().parse::<u32>() {
        Ok(value) => value,
        Err(_) => {
            eprintln!("Invalid {flag} value: {value}");
            print_usage_and_exit(2);
        }
    }
}

fn terrain_settings_from_options(options: &MinimalGlobeOptions) -> TerrainSettings {
    let settings = TerrainSettings::with_albedo().with_streaming_target_lod_count(options.max_lod);

    match env::var(STREAMING_CACHE_ROOT_ENV) {
        Ok(root) if !root.trim().is_empty() => settings.with_streaming_cache_root(root),
        _ if options.stream_online || options.stream_height => {
            settings.with_streaming_cache_root("streaming_cache")
        }
        _ => settings,
    }
}

fn streaming_settings_from_options(
    options: &MinimalGlobeOptions,
) -> Option<TerrainStreamingSettings> {
    if !options.stream_online && !options.stream_height {
        return None;
    }

    Some(if options.stream_height {
        TerrainStreamingSettings::online_imagery_and_height()
            .with_max_inflight_requests(DEFAULT_HEIGHT_STREAM_MAX_INFLIGHT)
    } else {
        TerrainStreamingSettings::online_imagery()
    })
}

fn env_u32(name: &str) -> Option<u32> {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
}

fn env_var_enabled(name: &str) -> bool {
    matches!(
        env::var(name).ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn imagery_provider_for_minimal() -> NasaGibsImageryProvider {
    match env::var(IMAGERY_PRESET_ENV).ok().as_deref() {
        Some("gibs_modis") => {
            info!("Minimal Globe imagery preset: gibs_modis");
            NasaGibsImageryProvider::new(NasaGibsImageryConfig::gibs_modis_true_color())
        }
        Some("eox_s2cloudless_2017") | None => {
            info!("Minimal Globe imagery preset: eox_s2cloudless_2017");
            NasaGibsImageryProvider::new(NasaGibsImageryConfig::eox_s2cloudless_2017())
        }
        Some(other) => {
            warn!(
                "Unknown TERRAIN_STREAM_IMAGERY_PRESET={other}. Falling back to eox_s2cloudless_2017."
            );
            NasaGibsImageryProvider::new(NasaGibsImageryConfig::eox_s2cloudless_2017())
        }
    }
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
    options: Res<MinimalGlobeOptions>,
) {
    let terrain_root = options.terrain_root.clone();
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
        "Controls: scroll or right-drag to zoom, left-drag to pan, middle-drag to orbit, T toggles fly camera. Use --max-lod N or {MAX_LOD_ENV}=N to change the refinement ceiling."
    );

    if let Ok(config) = TerrainConfig::load_file(&terrain_config_fs) {
        info!(
            "Minimal Globe target max LOD: {} (local terrain base lod_count={}).",
            options.max_lod.max(config.lod_count),
            config.lod_count
        );

        if options.stream_height && env::var("OPENTOPOGRAPHY_API_KEY").is_err() {
            warn!(
                "--stream-height was requested, but OPENTOPOGRAPHY_API_KEY is not set. The example will keep falling back to locally available height."
            );
        }

        if options.max_lod > config.lod_count {
            if options.stream_height {
                info!(
                    "Height streaming is enabled, so the renderer can refine beyond the local terrain asset up to LOD {}.",
                    options.max_lod
                );
            } else {
                warn!(
                    "Requested max LOD {} exceeds locally available lod_count={}. For actual terrain detail at that LOD, use --stream-height (or TERRAIN_STREAM_HEIGHT=1) with OPENTOPOGRAPHY_API_KEY, or point {} at a warmed cache.",
                    options.max_lod, config.lod_count, STREAMING_CACHE_ROOT_ENV
                );
            }
        }

        if terrain_root == DEFAULT_TERRAIN_ROOT && config.lod_count < 5 {
            if options.stream_height {
                info!(
                    "Bundled Earth local fallback is a coarse starter dataset (lod_count={}), but height streaming is enabled so close-pass terrain can refine beyond the bundled asset.",
                    config.lod_count
                );
            } else {
                warn!(
                    "Bundled Earth is a coarse starter dataset (lod_count={}). Steep relief like the Alps will look soft unless you use a higher-resolution terrain root, cached higher-LOD tiles, or TERRAIN_STREAM_HEIGHT=1 with OpenTopography.",
                    config.lod_count
                );
            }
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
