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
// LOD 10 gives ~78 m/pixel tiles (near OpenTopography AW3D30 native resolution).
const DEFAULT_MAX_LOD: u32 = 10;
const CAMERA_TARGET_LAT_ENV: &str = "MINIMAL_GLOBE_TARGET_LAT";
const CAMERA_TARGET_LON_ENV: &str = "MINIMAL_GLOBE_TARGET_LON";
const CAMERA_ALTITUDE_ENV: &str = "MINIMAL_GLOBE_CAMERA_ALTITUDE_M";
const CAMERA_BACKOFF_ENV: &str = "MINIMAL_GLOBE_CAMERA_BACKOFF_M";
// Default camera looks over the Himalaya — the most dramatic terrain on Earth.
const DEFAULT_TARGET_LAT_DEG: f64 = 27.988; // Everest region, Nepal
const DEFAULT_TARGET_LON_DEG: f64 = 86.925;
const DEFAULT_CAMERA_ALTITUDE_M: f32 = 10_000.0;
const DEFAULT_CAMERA_BACKOFF_M: f32 = 4_000.0;
// 8 concurrent height requests fills the streaming queue faster during initial tile warm-up.
const DEFAULT_HEIGHT_STREAM_MAX_INFLIGHT: usize = 8;

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
        // Load .env.opentopography.local if present (does not override already-set env vars).
        let _ = dotenvy::from_filename(".env.opentopography.local");

        let mut terrain_root = None;
        let mut max_lod = env_u32(MAX_LOD_ENV)
            .or_else(|| env_u32(STREAMING_MAX_LOD_ENV))
            .unwrap_or(DEFAULT_MAX_LOD);
        let mut stream_online = env_var_enabled(STREAM_ONLINE_ENV);
        let mut stream_height = env_var_enabled(STREAM_HEIGHT_ENV);

        // Auto-enable height streaming when the API key is available and the user has not
        // explicitly disabled it by setting TERRAIN_STREAM_HEIGHT=0/false/no.
        let api_key_present = env::var("OPENTOPOGRAPHY_API_KEY")
            .or_else(|_| env::var("OPEN_TOPOGRAPHY_API_KEY"))
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let height_disabled_explicitly = matches!(
            env::var(STREAM_HEIGHT_ENV).ok().as_deref(),
            Some("0") | Some("false") | Some("FALSE") | Some("no") | Some("NO")
        );
        if api_key_present && !height_disabled_explicitly {
            stream_height = true;
            stream_online = true;
        }

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
        "Minimal Globe — streams a live Earth terrain with satellite imagery and real elevation.\n\
         \n\
         USAGE:\n\
             cargo run --example minimal_globe [OPTIONS] [terrain_root]\n\
         \n\
         OPTIONS:\n\
             --stream-height         Enable elevation streaming via OpenTopography.\n\
                                     Auto-enabled when OPENTOPOGRAPHY_API_KEY is present.\n\
             --stream-online         Enable imagery-only streaming (no elevation).\n\
             --max-lod <N>           Maximum streamed LOD, default {DEFAULT_MAX_LOD}.\n\
             --terrain-root <PATH>   Asset-relative path to terrain folder,\n\
                                     default \"{DEFAULT_TERRAIN_ROOT}\".\n\
             -h, --help              Print this message.\n\
         \n\
         ENVIRONMENT:\n\
             OPENTOPOGRAPHY_API_KEY  Your OpenTopography API key. Place it in\n\
                                     .env.opentopography.local (auto-loaded) or set it\n\
                                     in your shell. Height streaming activates automatically.\n\
             {STREAM_HEIGHT_ENV}=0   Disable auto height streaming even if key is present.\n\
             {STREAM_ONLINE_ENV}=1   Force imagery-only streaming without a key.\n\
             {MAX_LOD_ENV}=N         Override max LOD via environment.\n\
             {STREAMING_CACHE_ROOT_ENV}=<path>  Override streaming tile cache path.\n\
             {IMAGERY_PRESET_ENV}=eox_s2cloudless_2017|gibs_modis  Choose imagery source.\n\
             {CAMERA_TARGET_LAT_ENV}=<deg>  Camera target latitude  (default: {DEFAULT_TARGET_LAT_DEG})\n\
             {CAMERA_TARGET_LON_ENV}=<deg>  Camera target longitude (default: {DEFAULT_TARGET_LON_DEG})\n\
             {CAMERA_ALTITUDE_ENV}=<m>      Camera altitude in metres\n\
             {CAMERA_BACKOFF_ENV}=<m>       Camera northward offset in metres\n\
         \n\
         QUICK START:\n\
             1. Add your OpenTopography API key to .env.opentopography.local:\n\
                    OPENTOPOGRAPHY_API_KEY=your-key-here\n\
             2. Run:\n\
                    cargo run --example minimal_globe\n\
             3. The camera opens over the Himalaya at 25 km altitude.\n\
                Height and imagery stream in progressively — mountains are\n\
                visible within about 30 seconds on a typical connection.\n\
         \n\
         CONTROLS:\n\
             Scroll / right-drag   Zoom in/out\n\
             Left-drag             Pan\n\
             Middle-drag           Orbit\n\
             T                     Toggle fly camera\n"
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
    // Always start with a focused view over terrain. Default is the Himalaya so that
    // elevation relief is immediately apparent once streaming tiles arrive.
    let lat_deg = env_f64(CAMERA_TARGET_LAT_ENV).unwrap_or(DEFAULT_TARGET_LAT_DEG);
    let lon_deg = env_f64(CAMERA_TARGET_LON_ENV).unwrap_or(DEFAULT_TARGET_LON_DEG);
    camera_transform_for_focus(
        lat_deg,
        lon_deg,
        env_f32(CAMERA_ALTITUDE_ENV, DEFAULT_CAMERA_ALTITUDE_M),
        env_f32(CAMERA_BACKOFF_ENV, DEFAULT_CAMERA_BACKOFF_M),
    )
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

    let api_key_set = env::var("OPENTOPOGRAPHY_API_KEY")
        .or_else(|_| env::var("OPEN_TOPOGRAPHY_API_KEY"))
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    if let Ok(config) = TerrainConfig::load_file(&terrain_config_fs) {
        let effective_max_lod = options.max_lod.max(config.lod_count);
        let imagery_status = if options.stream_online { "ON (streaming)" } else { "off (local only)" };
        let height_status = if options.stream_height && api_key_set {
            "ON (streaming via OpenTopography)"
        } else if options.stream_height && !api_key_set {
            "DISABLED — OPENTOPOGRAPHY_API_KEY not set"
        } else {
            "off (local only)"
        };
        let target_lat = env_f64(CAMERA_TARGET_LAT_ENV).unwrap_or(DEFAULT_TARGET_LAT_DEG);
        let target_lon = env_f64(CAMERA_TARGET_LON_ENV).unwrap_or(DEFAULT_TARGET_LON_DEG);

        info!(
            "\n  === Minimal Globe Configuration ===\n\n  \
             Terrain root:     {terrain_root}\n  \
             Base LODs:        {} (from terrain config)\n  \
             Streaming target: LOD {effective_max_lod}\n  \
             Imagery:          {imagery_status}\n  \
             Height:           {height_status}\n  \
             Camera target:    lat={target_lat:.3} lon={target_lon:.3} at {} m altitude\n\n  \
             Controls: scroll/right-drag=zoom, left-drag=pan, middle-drag=orbit, T=fly cam\n  \
             Run with --help for all options.",
            config.lod_count,
            env_f32(CAMERA_ALTITUDE_ENV, DEFAULT_CAMERA_ALTITUDE_M) as u32,
        );

        if options.stream_height && !api_key_set {
            warn!(
                "Height streaming requested but OPENTOPOGRAPHY_API_KEY is not set. \
                 Add it to .env.opentopography.local (auto-loaded) or set it in your shell. \
                 Run with --help for instructions."
            );
        }

        if options.max_lod > config.lod_count && !options.stream_height {
            warn!(
                "Max LOD {} exceeds the local terrain's lod_count={}. \
                 Terrain will look coarse beyond LOD {}. \
                 Add OPENTOPOGRAPHY_API_KEY to .env.opentopography.local for live height streaming.",
                options.max_lod, config.lod_count, config.lod_count
            );
        }

        if terrain_root == DEFAULT_TERRAIN_ROOT && config.lod_count < 5 && !options.stream_height {
            warn!(
                "Bundled Earth dataset is a coarse starter (lod_count={}). \
                 Mountains will look soft at close range without height streaming. \
                 Add OPENTOPOGRAPHY_API_KEY to .env.opentopography.local to stream real elevation.",
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

    // grid_size 32 gives 32×32 mesh vertices per tile — 4× more geometric detail than
    // the default 16, needed to represent ~78 m/pixel height data at LOD 10.
    let view_config = TerrainViewConfig {
        grid_size: 32,
        ..TerrainViewConfig::default()
    };

    commands.spawn_terrain(
        asset_server.load(terrain_config),
        view_config,
        SimpleTerrainMaterial::for_terrain(&asset_server, &mut loading_images, &terrain_root),
        view,
    );
}
