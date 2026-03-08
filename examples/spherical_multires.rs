use bevy::app::AppExit;
#[cfg(feature = "metal_capture")]
use bevy::diagnostic::FrameCount;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::ButtonInput;
use bevy::render::RenderApp;
use bevy::render::view::Msaa;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::shader::ShaderRef;
use bevy::time::Real;
use bevy::window::{PresentMode, WindowResolution};
use bevy::{math::DVec3, prelude::*, reflect::TypePath, render::render_resource::*};
use bevy_terrain::debug::DebugTerrain;
#[cfg(feature = "metal_capture")]
use bevy_terrain::debug::{FrameCapture, MetalCapturePlugin};
use bevy_terrain::math::{
    Coordinate,
    geodesy::{
        LlaHae, Ned, ecef_to_lla_hae, ned_to_ecef, renderer_local_to_lla_hae,
        unit_from_lat_lon_degrees,
    },
};
use bevy_terrain::perf::{TerrainPerfSnapshot, TerrainPerfTelemetry};
use bevy_terrain::prelude::*;
use big_space::prelude::{CellCoord, Grids};
use std::collections::VecDeque;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{collections::HashMap, env};

const RADIUS: f64 = 6371000.0;
const BASE_TERRAIN_CONFIG: &str = "terrains/earth/config.tc.ron";
const DEFAULT_OVERLAY_KEYS: &[&str] = &["swiss"];
const OVERLAY_ENV: &str = "MULTIRES_OVERLAYS";
const CAMERA_ALT_ENV: &str = "MULTIRES_CAMERA_ALT_M";
const CAMERA_BACKOFF_ENV: &str = "MULTIRES_CAMERA_BACKOFF_M";
const PRESENT_MODE_ENV: &str = "MULTIRES_PRESENT_MODE";
const BENCHMARK_OUTPUT_ENV: &str = "MULTIRES_BENCHMARK_OUTPUT";
const BENCHMARK_WARMUP_ENV: &str = "MULTIRES_BENCHMARK_WARMUP_SECONDS";
const BENCHMARK_DURATION_ENV: &str = "MULTIRES_BENCHMARK_DURATION_SECONDS";
const BENCHMARK_READY_TIMEOUT_ENV: &str = "MULTIRES_BENCHMARK_READY_TIMEOUT_SECONDS";
const BENCHMARK_SCENARIO_ENV: &str = "MULTIRES_BENCHMARK_SCENARIO";
const BENCHMARK_SWEEP_DEG_ENV: &str = "MULTIRES_BENCHMARK_SWEEP_DEG";
const BENCHMARK_SWEEP_PERIOD_ENV: &str = "MULTIRES_BENCHMARK_SWEEP_PERIOD_SECONDS";
const CAPTURE_DIR_ENV: &str = "MULTIRES_CAPTURE_DIR";
const CAPTURE_FRAMES_ENV: &str = "MULTIRES_CAPTURE_FRAMES";
#[cfg(feature = "metal_capture")]
const METAL_CAPTURE_FRAME_ENV: &str = "MULTIRES_METAL_CAPTURE_FRAME";
#[cfg(feature = "metal_capture")]
const METAL_CAPTURE_DIR_ENV: &str = "MULTIRES_METAL_CAPTURE_DIR";
const ENABLE_DEBUG_TOOLS_ENV: &str = "MULTIRES_ENABLE_DEBUG_TOOLS";
const ENABLE_CLICK_READOUT_ENV: &str = "MULTIRES_ENABLE_CLICK_READOUT";
const ENABLE_PERF_TITLE_ENV: &str = "MULTIRES_ENABLE_PERF_TITLE";
const UPLOAD_BUDGET_MB_ENV: &str = "MULTIRES_UPLOAD_BUDGET_MB";
const MSAA_SAMPLES_ENV: &str = "MULTIRES_MSAA_SAMPLES";
const TERRAIN_LIGHTING_ENV: &str = "MULTIRES_TERRAIN_LIGHTING";
const TERRAIN_MORPH_ENV: &str = "MULTIRES_TERRAIN_MORPH";
const TERRAIN_BLEND_ENV: &str = "MULTIRES_TERRAIN_BLEND";
const TERRAIN_SAMPLE_GRAD_ENV: &str = "MULTIRES_TERRAIN_SAMPLE_GRAD";
const TERRAIN_HIGH_PRECISION_ENV: &str = "MULTIRES_TERRAIN_HIGH_PRECISION";
const ENABLE_DRONE_ENV: &str = "MULTIRES_ENABLE_DRONE";
const DRONE_AGL_ENV: &str = "MULTIRES_DRONE_AGL_M";
const DRONE_RADIUS_ENV: &str = "MULTIRES_DRONE_ORBIT_RADIUS_M";
const DRONE_PERIOD_ENV: &str = "MULTIRES_DRONE_PERIOD_SECONDS";
const DRONE_SAMPLES_ENV: &str = "MULTIRES_DRONE_SAMPLES";
const DRONE_SIZE_ENV: &str = "MULTIRES_DRONE_SIZE_M";
const PERF_TITLE_PREFIX: &str = "SphericalMultires";
const CLICK_READOUT_PROMPT: &str =
    "Left click terrain to inspect latitude, longitude, and WGS84 HAE.";
const CLICK_COPY_PROMPT: &str = "Press Cmd+C or Ctrl+C to copy the last clicked coordinates.";
const CLICK_MARKER_RADIUS_M: f32 = 120.0;
const CLICK_MARKER_OFFSET_M: f64 = 80.0;

#[derive(Clone, Copy)]
struct OverlayPreset {
    config_path: &'static str,
    truth_source_raster_path: Option<&'static str>,
    label: &'static str,
    focus_lat_deg: f64,
    focus_lon_deg: f64,
}

#[derive(Resource)]
struct CameraFocus {
    label: String,
    lat_deg: f64,
    lon_deg: f64,
}

#[derive(Resource)]
struct PerfTitleState {
    timer: Timer,
    samples_ms: VecDeque<f64>,
}

impl Default for PerfTitleState {
    fn default() -> Self {
        Self {
            timer: Timer::from_seconds(1.0, TimerMode::Repeating),
            samples_ms: VecDeque::with_capacity(240),
        }
    }
}

#[derive(Resource)]
struct BenchmarkConfig {
    output_path: PathBuf,
    scenario_name: String,
    warmup_s: f64,
    duration_s: f64,
    ready_timeout_s: f64,
}

#[derive(Resource)]
struct BenchmarkCameraMotion {
    center_lat_deg: f64,
    center_lon_deg: f64,
    altitude_m: f32,
    backoff_m: f32,
    sweep_deg: f64,
    period_s: f64,
    elapsed_s: f64,
}

#[derive(Resource)]
struct CaptureConfig {
    output_dir: PathBuf,
    capture_frames: Vec<u32>,
}

#[derive(Clone)]
struct DroneOrbitSample {
    local_position: DVec3,
}

#[derive(Resource, Clone)]
struct DemoDroneOrbit {
    samples: Vec<DroneOrbitSample>,
    elapsed_s: f64,
    period_s: f64,
}

#[derive(Resource, Clone, Copy)]
struct RuntimeMode {
    benchmark_mode: bool,
    debug_tools_enabled: bool,
    click_readout_enabled: bool,
    perf_title_enabled: bool,
}

#[derive(Resource)]
struct ClickReadoutState {
    summary_line: String,
    detail_line: String,
    status_line: String,
    clipboard_payload: Option<String>,
}

impl Default for ClickReadoutState {
    fn default() -> Self {
        Self {
            summary_line: CLICK_READOUT_PROMPT.to_string(),
            detail_line: "Renderer local XYZ will appear after a terrain click.".to_string(),
            status_line: CLICK_COPY_PROMPT.to_string(),
            clipboard_payload: None,
        }
    }
}

impl ClickReadoutState {
    fn text(&self) -> String {
        format!(
            "{}\n{}\n{}",
            self.summary_line, self.detail_line, self.status_line
        )
    }
}

#[cfg(feature = "metal_capture")]
#[derive(Resource, Clone)]
struct MetalCaptureConfig {
    frame: u32,
    output_dir: PathBuf,
    label: String,
}

#[derive(Default)]
struct CaptureState {
    frame_index: u32,
    next_capture_idx: usize,
}

#[derive(Default)]
struct BenchmarkRuntime {
    ready_wait_s: f64,
    warmup_elapsed_s: f64,
    measure_elapsed_s: f64,
    status_log_elapsed_s: f64,
    measurement_window_started: bool,
    saw_ready_once: bool,
    ready_atlas_count: usize,
    ready_loaded_atlas_count: usize,
    ready_loaded_tile_total: usize,
    samples_ms: Vec<f64>,
    completed: bool,
}

struct BenchmarkSummary {
    scenario_name: String,
    overlays: String,
    present_mode: String,
    focus_label: String,
    focus_lat_deg: f64,
    focus_lon_deg: f64,
    benchmark_mode: bool,
    debug_tools_enabled: bool,
    perf_title_enabled: bool,
    ready_wait_s: f64,
    ready_atlas_count: usize,
    ready_loaded_atlas_count: usize,
    ready_loaded_tile_total: usize,
    warmup_s: f64,
    duration_s: f64,
    sample_count: usize,
    fps_mean: f64,
    frame_ms_mean: f64,
    frame_ms_min: f64,
    frame_ms_p50: f64,
    frame_ms_p90: f64,
    frame_ms_p95: f64,
    frame_ms_p99: f64,
    frame_ms_max: f64,
    frame_over_25ms_count: usize,
    frame_over_33ms_count: usize,
    frame_over_50ms_count: usize,
    latency_estimate_ms: f64,
    peak_rss_kib: u64,
    msaa_samples: u32,
    benchmark_sweep_deg: f64,
    benchmark_sweep_period_s: f64,
    drone_enabled: bool,
    terrain_lighting_enabled: bool,
    terrain_morph_enabled: bool,
    terrain_blend_enabled: bool,
    terrain_sample_grad_enabled: bool,
    terrain_high_precision_enabled: bool,
    hottest_phase_name: String,
    hottest_phase_mean_ms: f64,
    hottest_phase_p95_ms: f64,
    hottest_phase_max_ms: f64,
    upload_budget_bytes_per_frame: usize,
    phase_timings: TerrainPerfSnapshot,
    terrain_view_buffer_updates_total: u64,
    tile_tree_buffer_updates_total: u64,
    tile_tree_buffer_skipped_total: u64,
    tile_requests_total: u64,
    tile_releases_total: u64,
    canceled_pending_attachment_loads_total: u64,
    canceled_inflight_attachment_loads_total: u64,
    finished_attachment_loads_total: u64,
    upload_enqueued_attachment_tiles_total: u64,
    upload_enqueued_bytes_total: u64,
    upload_deferred_attachment_tiles_total: u64,
    peak_pending_attachment_queue: usize,
    peak_inflight_attachment_loads: usize,
    peak_upload_backlog_attachment_tiles: usize,
    canceled_stale_upload_attachment_tiles_total: u64,
}

#[derive(Component)]
struct PrimaryTerrainCamera;

#[derive(Component)]
struct DemoDrone;

#[derive(Component)]
struct ClickReadoutText;

#[derive(Component)]
struct ClickMarker;

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
    let present_mode = present_mode_from_env();
    let benchmark_mode = benchmark_mode_enabled();
    let debug_tools_enabled = env_bool(ENABLE_DEBUG_TOOLS_ENV, !benchmark_mode);
    let click_readout_enabled = env_bool(ENABLE_CLICK_READOUT_ENV, !benchmark_mode);
    let perf_title_enabled = env_bool(ENABLE_PERF_TITLE_ENV, !benchmark_mode);
    let terrain_debug = terrain_debug_from_env();
    let upload_budget_mb = env_usize(UPLOAD_BUDGET_MB_ENV, 24);
    let upload_budget_bytes_per_frame = if upload_budget_mb == 0 {
        0
    } else {
        upload_budget_mb * 1024 * 1024
    };

    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: WindowResolution::new(1920, 1080),
                    present_mode,
                    ..default()
                }),
                ..default()
            })
            .build()
            .disable::<TransformPlugin>(),
        TerrainPlugin,
        TerrainMaterialPlugin::<CustomMaterial>::default(),
        FrameTimeDiagnosticsPlugin::default(),
    ));
    app.insert_resource(terrain_debug.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(terrain_debug.clone());

    if debug_tools_enabled {
        app.add_plugins(TerrainDebugPlugin);
    }
    if debug_tools_enabled || click_readout_enabled {
        app.add_plugins(TerrainPickingPlugin);
    }
    #[cfg(feature = "metal_capture")]
    if metal_capture_config_from_env().is_some() && !debug_tools_enabled {
        app.add_plugins(MetalCapturePlugin);
    }

    app.insert_resource(
        TerrainSettings::new(vec!["albedo"])
            .with_upload_budget_bytes_per_frame(upload_budget_bytes_per_frame),
    )
    .insert_resource(RuntimeMode {
        benchmark_mode,
        debug_tools_enabled,
        click_readout_enabled,
        perf_title_enabled,
    })
    .insert_resource(ClickReadoutState::default())
    .insert_resource(PerfTitleState::default())
    .init_resource::<LoadingImages>()
    .add_systems(Startup, initialize)
    .add_systems(
        Update,
        (
            animate_benchmark_camera,
            animate_demo_drone,
            inspect_clicked_terrain_point,
            copy_click_readout_to_clipboard,
            update_click_readout_ui,
            capture_benchmark_frames,
            run_benchmark,
        ),
    );
    #[cfg(feature = "metal_capture")]
    if let Some(config) = metal_capture_config_from_env() {
        app.insert_resource(config)
            .add_systems(Update, schedule_metal_capture);
    }

    if perf_title_enabled {
        app.add_systems(Update, update_perf_title);
    }
    if !debug_tools_enabled {
        app.add_systems(Update, finish_loading_images_local);
    }

    app.run();
}

fn asset_exists(asset_path: &str) -> bool {
    let fs_path = format!("assets/{asset_path}");
    Path::new(&fs_path).is_file()
}

fn overlay_config_map() -> HashMap<&'static str, OverlayPreset> {
    HashMap::from([
        (
            "swiss",
            OverlayPreset {
                config_path: "terrains/swiss_highres/config.tc.ron",
                truth_source_raster_path: Some("source_data/swiss.tif"),
                label: "Swiss Alps",
                focus_lat_deg: 46.8,
                focus_lon_deg: 8.2,
            },
        ),
        (
            "saxony",
            OverlayPreset {
                config_path: "terrains/saxony_partial/config.tc.ron",
                truth_source_raster_path: None,
                label: "Saxony",
                focus_lat_deg: 50.9,
                focus_lon_deg: 13.5,
            },
        ),
        (
            "los",
            OverlayPreset {
                config_path: "terrains/los_highres/config.tc.ron",
                truth_source_raster_path: None,
                label: "Los Angeles",
                focus_lat_deg: 34.05,
                focus_lon_deg: -118.25,
            },
        ),
        (
            "srtm_n27e086",
            OverlayPreset {
                config_path: "terrains/srtm_n27e086/config.tc.ron",
                truth_source_raster_path: None,
                label: "Himalaya",
                focus_lat_deg: 27.9,
                focus_lon_deg: 86.9,
            },
        ),
        (
            "srtm_n35e139",
            OverlayPreset {
                config_path: "terrains/srtm_n35e139/config.tc.ron",
                truth_source_raster_path: None,
                label: "Tokyo",
                focus_lat_deg: 35.68,
                focus_lon_deg: 139.76,
            },
        ),
        (
            "srtm_n37e127",
            OverlayPreset {
                config_path: "terrains/srtm_n37e127/config.tc.ron",
                truth_source_raster_path: None,
                label: "Korea",
                focus_lat_deg: 37.57,
                focus_lon_deg: 126.98,
            },
        ),
        (
            "srtm_n39w077",
            OverlayPreset {
                config_path: "terrains/srtm_n39w077/config.tc.ron",
                truth_source_raster_path: None,
                label: "DC Region",
                focus_lat_deg: 39.0,
                focus_lon_deg: -77.0,
            },
        ),
        (
            "srtm_n51e000",
            OverlayPreset {
                config_path: "terrains/srtm_n51e000/config.tc.ron",
                truth_source_raster_path: None,
                label: "London",
                focus_lat_deg: 51.5,
                focus_lon_deg: 0.0,
            },
        ),
        (
            "srtm_s22w043",
            OverlayPreset {
                config_path: "terrains/srtm_s22w043/config.tc.ron",
                truth_source_raster_path: None,
                label: "Rio",
                focus_lat_deg: -22.9,
                focus_lon_deg: -43.2,
            },
        ),
        (
            "srtm_s33e151",
            OverlayPreset {
                config_path: "terrains/srtm_s33e151/config.tc.ron",
                truth_source_raster_path: None,
                label: "Sydney",
                focus_lat_deg: -33.87,
                focus_lon_deg: 151.21,
            },
        ),
    ])
}

fn env_f32(name: &str, default: f32) -> f32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

fn env_bool(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_u32(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn terrain_debug_from_env() -> DebugTerrain {
    let defaults = DebugTerrain::default();
    DebugTerrain {
        lighting: env_bool(TERRAIN_LIGHTING_ENV, defaults.lighting),
        morph: env_bool(TERRAIN_MORPH_ENV, defaults.morph),
        blend: env_bool(TERRAIN_BLEND_ENV, defaults.blend),
        sample_grad: env_bool(TERRAIN_SAMPLE_GRAD_ENV, defaults.sample_grad),
        high_precision: env_bool(TERRAIN_HIGH_PRECISION_ENV, defaults.high_precision),
        ..defaults
    }
}

fn sample_source_raster_wgs84(source_raster: &Path, lat_deg: f64, lon_deg: f64) -> Option<f32> {
    let output = Command::new("gdallocationinfo")
        .args([
            "-valonly",
            "-r",
            "bilinear",
            "-wgs84",
            &source_raster.display().to_string(),
            &format!("{lon_deg:.12}"),
            &format!("{lat_deg:.12}"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
}

fn build_demo_drone_orbit(preset: OverlayPreset) -> Option<DemoDroneOrbit> {
    if !env_bool(ENABLE_DRONE_ENV, !benchmark_mode_enabled()) {
        return None;
    }

    let truth_source = preset.truth_source_raster_path?;
    let source_raster = Path::new(truth_source);
    if !source_raster.is_file() {
        warn!(
            "Drone demo skipped for {}: truth source raster missing at {}",
            preset.label,
            source_raster.display()
        );
        return None;
    }

    let orbit_radius_m = env_f64(DRONE_RADIUS_ENV, 1_500.0).max(100.0);
    let commanded_agl_m = env_f32(DRONE_AGL_ENV, 250.0).max(25.0);
    let sample_count = env_usize(DRONE_SAMPLES_ENV, 96).max(16);
    let period_s = env_f64(DRONE_PERIOD_ENV, 18.0).max(2.0);

    let origin = LlaHae {
        lat_deg: preset.focus_lat_deg,
        lon_deg: preset.focus_lon_deg,
        hae_m: 0.0,
    };

    let mut samples = Vec::with_capacity(sample_count);
    for sample_index in 0..sample_count {
        let theta = std::f64::consts::TAU * sample_index as f64 / sample_count as f64;
        let ned = Ned {
            n_m: orbit_radius_m * theta.cos(),
            e_m: orbit_radius_m * theta.sin(),
            d_m: 0.0,
        };
        let lla = ecef_to_lla_hae(ned_to_ecef(ned, origin));
        let Some(ground_msl_m) =
            sample_source_raster_wgs84(source_raster, lla.lat_deg, lla.lon_deg)
        else {
            continue;
        };

        let vehicle_height_m = ground_msl_m + commanded_agl_m;
        let local_position = Coordinate::from_lat_lon_degrees(lla.lat_deg, lla.lon_deg)
            .local_position(TerrainShape::WGS84, vehicle_height_m);
        samples.push(DroneOrbitSample { local_position });
    }

    if samples.len() < 8 {
        warn!(
            "Drone demo skipped for {}: only {}/{} valid orbit samples from {}",
            preset.label,
            samples.len(),
            sample_count,
            source_raster.display()
        );
        return None;
    }

    info!(
        target: "perf",
        "drone demo enabled: label={} source={} orbit_radius_m={:.1} agl_m={:.1} period_s={:.1} samples={}",
        preset.label,
        source_raster.display(),
        orbit_radius_m,
        commanded_agl_m,
        period_s,
        samples.len()
    );

    Some(DemoDroneOrbit {
        samples,
        elapsed_s: 0.0,
        period_s,
    })
}

fn benchmark_mode_enabled() -> bool {
    env::var(BENCHMARK_OUTPUT_ENV)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn present_mode_from_env() -> PresentMode {
    match env::var(PRESENT_MODE_ENV)
        .unwrap_or_else(|_| "auto_vsync".to_string())
        .to_lowercase()
        .as_str()
    {
        "novsync" | "auto_novsync" | "auto-no-vsync" => PresentMode::AutoNoVsync,
        "fifo" => PresentMode::Fifo,
        "fifo_relaxed" | "fifo-relaxed" => PresentMode::FifoRelaxed,
        "immediate" => PresentMode::Immediate,
        "mailbox" => PresentMode::Mailbox,
        _ => PresentMode::AutoVsync,
    }
}

fn benchmark_config_from_env() -> Option<BenchmarkConfig> {
    let output = env::var(BENCHMARK_OUTPUT_ENV).ok()?;
    let output = output.trim();
    if output.is_empty() {
        return None;
    }

    let warmup_s = env_f64(BENCHMARK_WARMUP_ENV, 8.0).max(0.0);
    let duration_s = env_f64(BENCHMARK_DURATION_ENV, 20.0).max(1.0);
    let ready_timeout_s = env_f64(BENCHMARK_READY_TIMEOUT_ENV, 120.0).max(1.0);
    let scenario_name = env::var(BENCHMARK_SCENARIO_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unnamed".to_string());

    Some(BenchmarkConfig {
        output_path: PathBuf::from(output),
        scenario_name,
        warmup_s,
        duration_s,
        ready_timeout_s,
    })
}

#[cfg(feature = "metal_capture")]
fn metal_capture_config_from_env() -> Option<MetalCaptureConfig> {
    let frame = env::var(METAL_CAPTURE_FRAME_ENV)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let output_dir = env::var(METAL_CAPTURE_DIR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("captures"));
    let label = env::var(BENCHMARK_SCENARIO_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "metal_capture".to_string());

    Some(MetalCaptureConfig {
        frame,
        output_dir,
        label,
    })
}

fn capture_config_from_env() -> Option<CaptureConfig> {
    let output_dir = env::var(CAPTURE_DIR_ENV).ok()?;
    let output_dir = output_dir.trim();
    if output_dir.is_empty() {
        return None;
    }

    let capture_frames_raw =
        env::var(CAPTURE_FRAMES_ENV).unwrap_or_else(|_| "120,360,720".to_string());
    let mut capture_frames = capture_frames_raw
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.parse::<u32>().ok()
        })
        .collect::<Vec<_>>();
    capture_frames.sort_unstable();
    capture_frames.dedup();
    if capture_frames.is_empty() {
        capture_frames.push(120);
    }

    Some(CaptureConfig {
        output_dir: PathBuf::from(output_dir),
        capture_frames,
    })
}

fn camera_transform_for_focus(
    lat_deg: f64,
    lon_deg: f64,
    altitude_m: f32,
    backoff_m: f32,
) -> Transform {
    let n = unit_from_lat_lon_degrees(lat_deg, lon_deg)
        .as_vec3()
        .normalize();
    let target = n * RADIUS as f32;

    let mut east = Vec3::Y.cross(n);
    if east.length_squared() < 1e-6 {
        east = Vec3::Z.cross(n);
    }
    east = east.normalize();
    let north = n.cross(east).normalize();

    let camera_position = target + n * altitude_m + north * backoff_m + east * (0.25 * backoff_m);

    Transform::from_translation(camera_position).looking_at(target, n)
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
    click_readout: Res<ClickReadoutState>,
    mode: Res<RuntimeMode>,
    perf_telemetry: Res<TerrainPerfTelemetry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
) {
    let overlay_map = overlay_config_map();
    let selected_keys = selected_overlay_keys();

    let benchmark_config = benchmark_config_from_env();
    let benchmark_enabled = benchmark_config.is_some();
    perf_telemetry.set_enabled(benchmark_enabled);
    if benchmark_enabled {
        perf_telemetry.reset();
    }
    if let Some(config) = benchmark_config {
        info!(
            target: "perf",
            "benchmark enabled: output={} warmup_s={:.1} duration_s={:.1} ready_timeout_s={:.1}",
            config.output_path.display(),
            config.warmup_s,
            config.duration_s,
            config.ready_timeout_s
        );
        commands.insert_resource(config);
    }
    if let Some(config) = capture_config_from_env() {
        info!(
            target: "perf",
            "screenshot capture enabled: dir={} frames={:?}",
            config.output_dir.display(),
            config.capture_frames
        );
        commands.insert_resource(config);
    }

    let gradient = asset_server.load("textures/gradient1.png");
    images.load_image(
        &gradient,
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
    );

    let focus_preset = selected_keys
        .iter()
        .find_map(|key| overlay_map.get(key.as_str()).copied())
        .or_else(|| overlay_map.get(DEFAULT_OVERLAY_KEYS[0]).copied());

    let camera_altitude_m = env_f32(CAMERA_ALT_ENV, 90_000.0);
    let camera_backoff_m = env_f32(CAMERA_BACKOFF_ENV, 150_000.0);
    if benchmark_enabled {
        if let Some(preset) = focus_preset {
            let sweep_deg = env_f64(BENCHMARK_SWEEP_DEG_ENV, 8.0).max(0.0);
            let period_s = env_f64(BENCHMARK_SWEEP_PERIOD_ENV, 40.0).max(2.0);
            commands.insert_resource(BenchmarkCameraMotion {
                center_lat_deg: preset.focus_lat_deg,
                center_lon_deg: preset.focus_lon_deg,
                altitude_m: camera_altitude_m,
                backoff_m: camera_backoff_m,
                sweep_deg,
                period_s,
                elapsed_s: 0.0,
            });
            info!(
                target: "perf",
                "benchmark camera sweep enabled: center=({:.4},{:.4}) sweep_deg={:.2} period_s={:.1}",
                preset.focus_lat_deg,
                preset.focus_lon_deg,
                sweep_deg,
                period_s
            );
        }
    }

    info!(
        target: "perf",
        "runtime_mode benchmark_mode={} debug_tools_enabled={} click_readout_enabled={} perf_title_enabled={}",
        mode.benchmark_mode,
        mode.debug_tools_enabled,
        mode.click_readout_enabled,
        mode.perf_title_enabled
    );

    if !mode.debug_tools_enabled {
        commands.spawn((
            DirectionalLight {
                illuminance: 5000.0,
                ..default()
            },
            Transform::from_xyz(-1.0, 1.0, -3.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.insert_resource(GlobalAmbientLight {
            brightness: 100.0,
            ..default()
        });
    }

    if mode.click_readout_enabled {
        commands.spawn((
            Text::new(click_readout.text()),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
            ClickReadoutText,
        ));
    }

    let camera_transform = if let Some(preset) = focus_preset {
        commands.insert_resource(CameraFocus {
            label: preset.label.to_string(),
            lat_deg: preset.focus_lat_deg,
            lon_deg: preset.focus_lon_deg,
        });

        camera_transform_for_focus(
            preset.focus_lat_deg,
            preset.focus_lon_deg,
            camera_altitude_m,
            camera_backoff_m,
        )
    } else {
        Transform::from_translation(-Vec3::X * RADIUS as f32 * 3.0).looking_to(Vec3::X, Vec3::Y)
    };

    let demo_drone_orbit = focus_preset.and_then(build_demo_drone_orbit);
    if let Some(drone_orbit) = demo_drone_orbit.clone() {
        commands.insert_resource(drone_orbit);
    }
    let msaa_samples = env_u32(MSAA_SAMPLES_ENV, 4);
    let click_marker_material = mode.click_readout_enabled.then(|| {
        standard_materials.add(StandardMaterial {
            base_color: Color::srgb(0.98, 0.92, 0.12),
            emissive: LinearRgba::rgb(6.0, 5.2, 0.8),
            unlit: true,
            ..default()
        })
    });
    let click_marker_mesh = mode
        .click_readout_enabled
        .then(|| meshes.add(Sphere::new(CLICK_MARKER_RADIUS_M).mesh().ico(5).unwrap()));

    let mut view = Entity::PLACEHOLDER;
    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                camera_transform,
                PrimaryTerrainCamera,
                Msaa::from_samples(msaa_samples),
                DebugCameraController::new(RADIUS),
                OrbitalCameraController::default(),
            ))
            .id();

        if let (Some(click_marker_mesh), Some(click_marker_material)) =
            (click_marker_mesh.as_ref(), click_marker_material.as_ref())
        {
            root.spawn_spatial((
                CellCoord::default(),
                Transform::from_translation(Vec3::ZERO),
                Visibility::Hidden,
                ClickMarker,
                Mesh3d(click_marker_mesh.clone()),
                MeshMaterial3d(click_marker_material.clone()),
            ));
        }

        if let Some(drone_orbit) = demo_drone_orbit.as_ref() {
            let grid = Grid::default();
            let drone_radius_m = env_f32(DRONE_SIZE_ENV, 180.0).max(25.0);
            let trail_radius_m = (0.45 * drone_radius_m).max(12.0);

            let drone_material = standard_materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.45, 0.05),
                emissive: LinearRgba::rgb(4.0, 1.2, 0.2),
                unlit: true,
                ..default()
            });
            let trail_material = standard_materials.add(StandardMaterial {
                base_color: Color::srgba(0.15, 0.95, 1.0, 0.55),
                emissive: LinearRgba::rgb(0.2, 1.0, 1.4),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });

            for sample in drone_orbit
                .samples
                .iter()
                .step_by((drone_orbit.samples.len() / 20).max(1))
            {
                let (marker_cell, marker_translation) =
                    grid.translation_to_grid(sample.local_position);
                root.spawn_spatial((
                    marker_cell,
                    Transform::from_translation(marker_translation),
                    Mesh3d(meshes.add(Sphere::new(trail_radius_m).mesh().ico(4).unwrap())),
                    MeshMaterial3d(trail_material.clone()),
                ));
            }

            if let Some(first_sample) = drone_orbit.samples.first() {
                let (drone_cell, drone_translation) =
                    grid.translation_to_grid(first_sample.local_position);
                root.spawn_spatial((
                    drone_cell,
                    Transform::from_translation(drone_translation),
                    DemoDrone,
                    Mesh3d(meshes.add(Sphere::new(drone_radius_m).mesh().ico(5).unwrap())),
                    MeshMaterial3d(drone_material),
                ));
            }
        }
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
        let Some(&preset) = overlay_map.get(key.as_str()) else {
            warn!("Unknown overlay key '{key}', skipping.");
            continue;
        };

        if !asset_exists(preset.config_path) {
            warn!(
                "Overlay config missing at '{}', skipping.",
                preset.config_path
            );
            continue;
        }

        commands.spawn_terrain(
            asset_server.load(preset.config_path),
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
    if let Some(preset) = focus_preset {
        info!(
            "Camera focus: {} at lat={:.4}, lon={:.4}, alt={}m, backoff={}m",
            preset.label,
            preset.focus_lat_deg,
            preset.focus_lon_deg,
            camera_altitude_m,
            camera_backoff_m,
        );
    }
}

fn animate_benchmark_camera(
    time: Res<Time<Real>>,
    grids: Grids,
    motion: Option<ResMut<BenchmarkCameraMotion>>,
    mut cameras: Query<(Entity, &mut Transform, &mut CellCoord), With<PrimaryTerrainCamera>>,
) {
    let Some(mut motion) = motion else {
        return;
    };
    if motion.sweep_deg <= f64::EPSILON {
        return;
    }

    motion.elapsed_s += time.delta_secs_f64();
    let phase = std::f64::consts::TAU * (motion.elapsed_s / motion.period_s);

    // Sweep enough to cross overlay boundaries so both high-res and base terrain appear.
    let lat_deg = motion.center_lat_deg + (0.55 * motion.sweep_deg) * (phase * 0.73).sin();
    let lon_deg = motion.center_lon_deg + motion.sweep_deg * phase.cos();
    let desired = camera_transform_for_focus(lat_deg, lon_deg, motion.altitude_m, motion.backoff_m);

    for (entity, mut camera_transform, mut camera_cell) in &mut cameras {
        let Some(grid) = grids.parent_grid(entity) else {
            continue;
        };
        let (new_cell, new_translation) = grid.translation_to_grid(desired.translation.as_dvec3());
        *camera_cell = new_cell;
        camera_transform.translation = new_translation;
        camera_transform.rotation = desired.rotation;
    }
}

fn animate_demo_drone(
    time: Res<Time<Real>>,
    grids: Grids,
    orbit: Option<ResMut<DemoDroneOrbit>>,
    mut drones: Query<(Entity, &mut Transform, &mut CellCoord), With<DemoDrone>>,
) {
    let Some(mut orbit) = orbit else {
        return;
    };
    if orbit.samples.len() < 2 {
        return;
    }

    orbit.elapsed_s += time.delta_secs_f64();
    let phase = (orbit.elapsed_s / orbit.period_s).rem_euclid(1.0);
    let sample_f = phase * orbit.samples.len() as f64;
    let start_index = sample_f.floor() as usize % orbit.samples.len();
    let end_index = (start_index + 1) % orbit.samples.len();
    let t = sample_f.fract();

    let start = orbit.samples[start_index].local_position;
    let end = orbit.samples[end_index].local_position;
    let local_position = start.lerp(end, t);

    for (entity, mut transform, mut cell) in &mut drones {
        let Some(grid) = grids.parent_grid(entity) else {
            continue;
        };
        let (new_cell, new_translation) = grid.translation_to_grid(local_position);
        *cell = new_cell;
        transform.translation = new_translation;
    }
}

fn inspect_clicked_terrain_point(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mode: Res<RuntimeMode>,
    grids: Grids,
    cameras: Query<(Entity, &PickingData), With<PrimaryTerrainCamera>>,
    mut click_markers: Query<(&mut Transform, &mut CellCoord, &mut Visibility), With<ClickMarker>>,
    mut click_readout: ResMut<ClickReadoutState>,
) {
    if !mode.click_readout_enabled || !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }

    match cameras.single() {
        Ok((entity, picking_data)) => match (picking_data.translation, grids.parent_grid(entity)) {
            (Some(hit_translation), Some(grid)) => {
                let local_position = grid.grid_position_double(
                    &picking_data.cell,
                    &Transform::from_translation(hit_translation),
                );
                let marker_position =
                    local_position + local_position.normalize_or_zero() * CLICK_MARKER_OFFSET_M;
                let lla = renderer_local_to_lla_hae(local_position);

                if let Ok((mut marker_transform, mut marker_cell, mut marker_visibility)) =
                    click_markers.single_mut()
                {
                    let (new_cell, new_translation) = grid.translation_to_grid(marker_position);
                    *marker_cell = new_cell;
                    marker_transform.translation = new_translation;
                    *marker_visibility = Visibility::Visible;
                }

                info!(
                    target: "click",
                    "terrain_click lat_deg={:.8} lon_deg={:.8} hae_m={:.3} local_x_m={:.3} local_y_m={:.3} local_z_m={:.3}",
                    lla.lat_deg,
                    lla.lon_deg,
                    lla.hae_m,
                    local_position.x,
                    local_position.y,
                    local_position.z
                );

                click_readout.summary_line = format!(
                    "Lat {:.8} deg | Lon {:.8} deg | WGS84 HAE {:.3} m",
                    lla.lat_deg, lla.lon_deg, lla.hae_m
                );
                click_readout.detail_line = format!(
                    "Renderer local XYZ = ({:.3}, {:.3}, {:.3}) m",
                    local_position.x, local_position.y, local_position.z
                );
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
                click_readout.clipboard_payload = Some(format!(
                    concat!(
                        "lat_deg={:.8}\n",
                        "lon_deg={:.8}\n",
                        "wgs84_hae_m={:.3}\n",
                        "renderer_local_x_m={:.3}\n",
                        "renderer_local_y_m={:.3}\n",
                        "renderer_local_z_m={:.3}\n"
                    ),
                    lla.lat_deg,
                    lla.lon_deg,
                    lla.hae_m,
                    local_position.x,
                    local_position.y,
                    local_position.z
                ));
            }
            (None, _) => {
                if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                    *marker_visibility = Visibility::Hidden;
                }
                click_readout.summary_line = "No terrain hit under cursor.".to_string();
                click_readout.detail_line =
                    "Renderer local XYZ is only available for valid terrain hits.".to_string();
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
            }
            (_, None) => {
                if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                    *marker_visibility = Visibility::Hidden;
                }
                click_readout.summary_line =
                    "No terrain grid available for click inspection.".to_string();
                click_readout.detail_line =
                    "Renderer local XYZ is only available for valid terrain hits.".to_string();
                click_readout.status_line = CLICK_COPY_PROMPT.to_string();
            }
        },
        Err(_) => {
            if let Ok((_, _, mut marker_visibility)) = click_markers.single_mut() {
                *marker_visibility = Visibility::Hidden;
            }
            click_readout.summary_line =
                "No primary terrain camera available for click inspection.".to_string();
            click_readout.detail_line =
                "Renderer local XYZ is only available for valid terrain hits.".to_string();
            click_readout.status_line = CLICK_COPY_PROMPT.to_string();
        }
    }
}

fn copy_click_readout_to_clipboard(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<RuntimeMode>,
    mut click_readout: ResMut<ClickReadoutState>,
) {
    if !mode.click_readout_enabled || !copy_shortcut_pressed(&keyboard) {
        return;
    }

    let Some(payload) = click_readout.clipboard_payload.clone() else {
        click_readout.status_line = "No clicked coordinates available to copy yet.".to_string();
        return;
    };

    match copy_text_to_clipboard(&payload) {
        Ok(()) => {
            click_readout.status_line = "Copied last clicked coordinates to clipboard.".to_string();
        }
        Err(error) => {
            click_readout.status_line = format!("Clipboard copy failed: {error}");
            warn!(target: "click", "clipboard copy failed: {error}");
        }
    }
}

fn update_click_readout_ui(
    mode: Res<RuntimeMode>,
    click_readout: Res<ClickReadoutState>,
    mut readout_text: Query<&mut Text, With<ClickReadoutText>>,
) {
    if !mode.click_readout_enabled || !click_readout.is_changed() {
        return;
    }

    for mut text in &mut readout_text {
        *text = Text::new(click_readout.text());
    }
}

fn copy_shortcut_pressed(keyboard: &ButtonInput<KeyCode>) -> bool {
    keyboard.just_pressed(KeyCode::KeyC)
        && (keyboard.pressed(KeyCode::SuperLeft)
            || keyboard.pressed(KeyCode::SuperRight)
            || keyboard.pressed(KeyCode::ControlLeft)
            || keyboard.pressed(KeyCode::ControlRight))
}

fn copy_text_to_clipboard(text: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|error| format!("failed to spawn pbcopy: {error}"))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open pbcopy stdin".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|error| format!("failed to write pbcopy stdin: {error}"))?;
        drop(stdin);
        let status = child
            .wait()
            .map_err(|error| format!("failed to wait for pbcopy: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("pbcopy exited with status {status}"))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err("clipboard copy is only implemented for macOS in this demo".to_string())
    }
}

#[cfg(feature = "metal_capture")]
fn schedule_metal_capture(
    frames: Res<FrameCount>,
    config: Res<MetalCaptureConfig>,
    mut capture: ResMut<FrameCapture>,
) {
    let should_capture = frames.0 == config.frame;
    capture.capture = should_capture;
    if should_capture {
        capture.output_dir = Some(config.output_dir.clone());
        capture.label = Some(config.label.clone());
        info!(
            target: "perf",
            "requesting metal capture at frame {} -> {}",
            config.frame,
            config.output_dir.display()
        );
    }
}

fn finish_loading_images_local(
    asset_server: Res<AssetServer>,
    mut loading_images: ResMut<LoadingImages>,
    mut images: ResMut<Assets<Image>>,
) {
    loading_images.finalize_ready_images(&asset_server, &mut images);
}

fn capture_benchmark_frames(
    mut commands: Commands,
    capture: Option<Res<CaptureConfig>>,
    mut state: Local<CaptureState>,
) {
    let Some(capture) = capture else {
        return;
    };
    if state.next_capture_idx >= capture.capture_frames.len() {
        return;
    }

    state.frame_index = state.frame_index.saturating_add(1);
    while state.next_capture_idx < capture.capture_frames.len()
        && state.frame_index >= capture.capture_frames[state.next_capture_idx]
    {
        let frame = capture.capture_frames[state.next_capture_idx];
        state.next_capture_idx += 1;

        let path = capture.output_dir.join(format!("frame_{frame:06}.png"));
        if let Some(parent) = path.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            error!(
                target: "perf",
                "failed to create capture output directory {}: {error}",
                parent.display()
            );
            continue;
        }

        info!(
            target: "perf",
            "capturing screenshot at frame {} -> {}",
            frame,
            path.display()
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}

fn update_perf_title(
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut state: ResMut<PerfTitleState>,
    focus: Option<Res<CameraFocus>>,
) {
    let smoothed_fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed());
    let smoothed_frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed());

    let frame_ms_sample = smoothed_frame_ms.unwrap_or(time.delta_secs_f64() * 1000.0);
    state.samples_ms.push_back(frame_ms_sample);
    while state.samples_ms.len() > 240 {
        state.samples_ms.pop_front();
    }

    if !state.timer.tick(time.delta()).just_finished() {
        return;
    }

    let len = state.samples_ms.len().max(1);
    let avg_ms = state.samples_ms.iter().sum::<f64>() / len as f64;

    let mut sorted = state.samples_ms.iter().copied().collect::<Vec<_>>();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let p95_index = ((sorted.len().saturating_sub(1)) as f64 * 0.95).round() as usize;
    let p95_ms = sorted.get(p95_index).copied().unwrap_or(avg_ms);

    let fps = smoothed_fps.unwrap_or_else(|| if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 });
    let frame_ms = smoothed_frame_ms.unwrap_or(avg_ms);
    let latency_estimate_ms = frame_ms.max(p95_ms);

    let focus_text = focus
        .as_ref()
        .map(|focus| {
            format!(
                "{} ({:.3},{:.3})",
                focus.label, focus.lat_deg, focus.lon_deg
            )
        })
        .unwrap_or_else(|| "global".to_string());

    let title = format!(
        "{PERF_TITLE_PREFIX} | focus {focus_text} | FPS {fps:.1} | frame {frame_ms:.2} ms | p95 {p95_ms:.2} ms | latency~ {latency_estimate_ms:.2} ms"
    );
    for mut window in &mut windows {
        window.title = title.clone();
    }

    info!(
        target: "perf",
        "focus={focus_text} fps={fps:.1} frame_ms={frame_ms:.2} p95_ms={p95_ms:.2} latency_est_ms={latency_estimate_ms:.2}"
    );
}

fn compute_percentile(sorted_samples: &[f64], percentile: f64) -> f64 {
    if sorted_samples.is_empty() {
        return 0.0;
    }
    let index = ((sorted_samples.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    sorted_samples[index]
}

fn peak_rss_kib() -> u64 {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        // SAFETY: `getrusage` initializes the provided `rusage` on success.
        let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        if result == 0 {
            let usage = unsafe { usage.assume_init() };
            #[cfg(target_os = "macos")]
            {
                return (usage.ru_maxrss as u64).saturating_div(1024);
            }
            #[cfg(not(target_os = "macos"))]
            {
                return usage.ru_maxrss as u64;
            }
        }
    }
    0
}

fn phase_timings_json_fragment(snapshot: &TerrainPerfSnapshot) -> String {
    let mut lines = Vec::new();
    for phase in &snapshot.phase_timings {
        let name = phase.name.replace('"', "'");
        lines.push(format!(
            "    \"{name}\": {{ \"sample_count\": {sample_count}, \"mean_ms\": {mean_ms:.6}, \"p95_ms\": {p95_ms:.6}, \"p99_ms\": {p99_ms:.6}, \"max_ms\": {max_ms:.6} }}",
            name = name,
            sample_count = phase.sample_count,
            mean_ms = phase.mean_ms,
            p95_ms = phase.p95_ms,
            p99_ms = phase.p99_ms,
            max_ms = phase.max_ms,
        ));
    }

    if lines.is_empty() {
        "{}".to_string()
    } else {
        format!("{{\n{}\n  }}", lines.join(",\n"))
    }
}

fn compute_summary(
    config: &BenchmarkConfig,
    samples_ms: &[f64],
    ready_wait_s: f64,
    focus: Option<&CameraFocus>,
    mode: &RuntimeMode,
    debug: &DebugTerrain,
    runtime: &BenchmarkRuntime,
    settings: &TerrainSettings,
    phase_timings: TerrainPerfSnapshot,
    tile_tree_perf_counters: impl IntoIterator<Item = TileTreePerfCounters>,
    tile_atlas_perf_counters: impl IntoIterator<Item = TileAtlasPerfCounters>,
) -> Option<BenchmarkSummary> {
    if samples_ms.is_empty() {
        return None;
    }

    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let sample_count = sorted.len();
    let sum = sorted.iter().sum::<f64>();
    let frame_ms_mean = sum / sample_count as f64;
    let frame_ms_min = *sorted.first().unwrap_or(&0.0);
    let frame_ms_max = *sorted.last().unwrap_or(&0.0);
    let frame_ms_p50 = compute_percentile(&sorted, 0.50);
    let frame_ms_p90 = compute_percentile(&sorted, 0.90);
    let frame_ms_p95 = compute_percentile(&sorted, 0.95);
    let frame_ms_p99 = compute_percentile(&sorted, 0.99);
    let frame_over_25ms_count = samples_ms
        .iter()
        .filter(|&&frame_ms| frame_ms > 25.0)
        .count();
    let frame_over_33ms_count = samples_ms
        .iter()
        .filter(|&&frame_ms| frame_ms > 33.0)
        .count();
    let frame_over_50ms_count = samples_ms
        .iter()
        .filter(|&&frame_ms| frame_ms > 50.0)
        .count();
    let fps_mean = if frame_ms_mean > 0.0 {
        1000.0 / frame_ms_mean
    } else {
        0.0
    };

    let (focus_label, focus_lat_deg, focus_lon_deg) = if let Some(focus) = focus {
        (focus.label.clone(), focus.lat_deg, focus.lon_deg)
    } else {
        ("global".to_string(), 0.0, 0.0)
    };
    let benchmark_sweep_deg = env_f64(BENCHMARK_SWEEP_DEG_ENV, 8.0).max(0.0);
    let benchmark_sweep_period_s = env_f64(BENCHMARK_SWEEP_PERIOD_ENV, 40.0).max(2.0);
    let hottest_phase = phase_timings.hottest_by_p95().cloned();

    let mut tile_tree_perf = TileTreePerfCounters::default();
    for counters in tile_tree_perf_counters {
        tile_tree_perf.terrain_view_buffer_updates_total +=
            counters.terrain_view_buffer_updates_total;
        tile_tree_perf.tile_tree_buffer_updates_total += counters.tile_tree_buffer_updates_total;
        tile_tree_perf.tile_tree_buffer_skipped_total += counters.tile_tree_buffer_skipped_total;
    }

    let mut perf = TileAtlasPerfCounters::default();
    for counters in tile_atlas_perf_counters {
        perf.tile_requests_total += counters.tile_requests_total;
        perf.tile_releases_total += counters.tile_releases_total;
        perf.canceled_pending_attachment_loads_total +=
            counters.canceled_pending_attachment_loads_total;
        perf.canceled_inflight_attachment_loads_total +=
            counters.canceled_inflight_attachment_loads_total;
        perf.canceled_stale_upload_attachment_tiles_total +=
            counters.canceled_stale_upload_attachment_tiles_total;
        perf.finished_attachment_loads_total += counters.finished_attachment_loads_total;
        perf.upload_enqueued_attachment_tiles_total +=
            counters.upload_enqueued_attachment_tiles_total;
        perf.upload_enqueued_bytes_total += counters.upload_enqueued_bytes_total;
        perf.upload_deferred_attachment_tiles_total +=
            counters.upload_deferred_attachment_tiles_total;
        perf.peak_pending_attachment_queue = perf
            .peak_pending_attachment_queue
            .max(counters.peak_pending_attachment_queue);
        perf.peak_inflight_attachment_loads = perf
            .peak_inflight_attachment_loads
            .max(counters.peak_inflight_attachment_loads);
        perf.peak_upload_backlog_attachment_tiles = perf
            .peak_upload_backlog_attachment_tiles
            .max(counters.peak_upload_backlog_attachment_tiles);
    }

    Some(BenchmarkSummary {
        scenario_name: config.scenario_name.clone(),
        overlays: env::var(OVERLAY_ENV).unwrap_or_else(|_| DEFAULT_OVERLAY_KEYS.join(",")),
        present_mode: env::var(PRESENT_MODE_ENV).unwrap_or_else(|_| "auto_vsync".to_string()),
        focus_label,
        focus_lat_deg,
        focus_lon_deg,
        benchmark_mode: mode.benchmark_mode,
        debug_tools_enabled: mode.debug_tools_enabled,
        perf_title_enabled: mode.perf_title_enabled,
        ready_wait_s,
        ready_atlas_count: runtime.ready_atlas_count,
        ready_loaded_atlas_count: runtime.ready_loaded_atlas_count,
        ready_loaded_tile_total: runtime.ready_loaded_tile_total,
        warmup_s: config.warmup_s,
        duration_s: config.duration_s,
        sample_count,
        fps_mean,
        frame_ms_mean,
        frame_ms_min,
        frame_ms_p50,
        frame_ms_p90,
        frame_ms_p95,
        frame_ms_p99,
        frame_ms_max,
        frame_over_25ms_count,
        frame_over_33ms_count,
        frame_over_50ms_count,
        latency_estimate_ms: frame_ms_p95,
        peak_rss_kib: peak_rss_kib(),
        msaa_samples: env_u32(MSAA_SAMPLES_ENV, 4),
        benchmark_sweep_deg,
        benchmark_sweep_period_s,
        drone_enabled: env_bool(ENABLE_DRONE_ENV, !benchmark_mode_enabled()),
        terrain_lighting_enabled: debug.lighting,
        terrain_morph_enabled: debug.morph,
        terrain_blend_enabled: debug.blend,
        terrain_sample_grad_enabled: debug.sample_grad,
        terrain_high_precision_enabled: debug.high_precision,
        hottest_phase_name: hottest_phase
            .as_ref()
            .map(|phase| phase.name.clone())
            .unwrap_or_else(|| "none".to_string()),
        hottest_phase_mean_ms: hottest_phase
            .as_ref()
            .map(|phase| phase.mean_ms)
            .unwrap_or(0.0),
        hottest_phase_p95_ms: hottest_phase
            .as_ref()
            .map(|phase| phase.p95_ms)
            .unwrap_or(0.0),
        hottest_phase_max_ms: hottest_phase
            .as_ref()
            .map(|phase| phase.max_ms)
            .unwrap_or(0.0),
        upload_budget_bytes_per_frame: settings.upload_budget_bytes_per_frame,
        phase_timings,
        terrain_view_buffer_updates_total: tile_tree_perf.terrain_view_buffer_updates_total,
        tile_tree_buffer_updates_total: tile_tree_perf.tile_tree_buffer_updates_total,
        tile_tree_buffer_skipped_total: tile_tree_perf.tile_tree_buffer_skipped_total,
        tile_requests_total: perf.tile_requests_total,
        tile_releases_total: perf.tile_releases_total,
        canceled_pending_attachment_loads_total: perf.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total: perf.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total: perf.finished_attachment_loads_total,
        upload_enqueued_attachment_tiles_total: perf.upload_enqueued_attachment_tiles_total,
        upload_enqueued_bytes_total: perf.upload_enqueued_bytes_total,
        upload_deferred_attachment_tiles_total: perf.upload_deferred_attachment_tiles_total,
        peak_pending_attachment_queue: perf.peak_pending_attachment_queue,
        peak_inflight_attachment_loads: perf.peak_inflight_attachment_loads,
        peak_upload_backlog_attachment_tiles: perf.peak_upload_backlog_attachment_tiles,
        canceled_stale_upload_attachment_tiles_total: perf
            .canceled_stale_upload_attachment_tiles_total,
    })
}

fn write_benchmark_outputs(
    path: &Path,
    summary: &BenchmarkSummary,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let mut json_path = path.to_path_buf();
    json_path.set_extension("json");
    let mut csv_path = path.to_path_buf();
    csv_path.set_extension("csv");

    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let scenario_name = summary.scenario_name.replace('"', "'");
    let overlays = summary.overlays.replace('"', "'");
    let present_mode = summary.present_mode.replace('"', "'");
    let focus_label = summary.focus_label.replace('"', "'");
    let hottest_phase_name = summary.hottest_phase_name.replace('"', "'");
    let phase_timings_json = phase_timings_json_fragment(&summary.phase_timings);
    let json = format!(
        concat!(
            "{{\n",
            "  \"scenario_name\": \"{scenario_name}\",\n",
            "  \"overlays\": \"{overlays}\",\n",
            "  \"present_mode\": \"{present_mode}\",\n",
            "  \"focus_label\": \"{focus_label}\",\n",
            "  \"focus_lat_deg\": {focus_lat_deg:.6},\n",
            "  \"focus_lon_deg\": {focus_lon_deg:.6},\n",
            "  \"benchmark_mode\": {benchmark_mode},\n",
            "  \"debug_tools_enabled\": {debug_tools_enabled},\n",
            "  \"perf_title_enabled\": {perf_title_enabled},\n",
            "  \"ready_wait_s\": {ready_wait_s:.3},\n",
            "  \"ready_atlas_count\": {ready_atlas_count},\n",
            "  \"ready_loaded_atlas_count\": {ready_loaded_atlas_count},\n",
            "  \"ready_loaded_tile_total\": {ready_loaded_tile_total},\n",
            "  \"warmup_s\": {warmup_s:.3},\n",
            "  \"duration_s\": {duration_s:.3},\n",
            "  \"sample_count\": {sample_count},\n",
            "  \"fps_mean\": {fps_mean:.4},\n",
            "  \"frame_ms_mean\": {frame_ms_mean:.6},\n",
            "  \"frame_ms_min\": {frame_ms_min:.6},\n",
            "  \"frame_ms_p50\": {frame_ms_p50:.6},\n",
            "  \"frame_ms_p90\": {frame_ms_p90:.6},\n",
            "  \"frame_ms_p95\": {frame_ms_p95:.6},\n",
            "  \"frame_ms_p99\": {frame_ms_p99:.6},\n",
            "  \"frame_ms_max\": {frame_ms_max:.6},\n",
            "  \"frame_over_25ms_count\": {frame_over_25ms_count},\n",
            "  \"frame_over_33ms_count\": {frame_over_33ms_count},\n",
            "  \"frame_over_50ms_count\": {frame_over_50ms_count},\n",
            "  \"latency_estimate_ms\": {latency_estimate_ms:.6},\n",
            "  \"peak_rss_kib\": {peak_rss_kib},\n",
            "  \"msaa_samples\": {msaa_samples},\n",
            "  \"benchmark_sweep_deg\": {benchmark_sweep_deg:.3},\n",
            "  \"benchmark_sweep_period_s\": {benchmark_sweep_period_s:.3},\n",
            "  \"drone_enabled\": {drone_enabled},\n",
            "  \"terrain_lighting_enabled\": {terrain_lighting_enabled},\n",
            "  \"terrain_morph_enabled\": {terrain_morph_enabled},\n",
            "  \"terrain_blend_enabled\": {terrain_blend_enabled},\n",
            "  \"terrain_sample_grad_enabled\": {terrain_sample_grad_enabled},\n",
            "  \"terrain_high_precision_enabled\": {terrain_high_precision_enabled},\n",
            "  \"hottest_phase_name\": \"{hottest_phase_name}\",\n",
            "  \"hottest_phase_mean_ms\": {hottest_phase_mean_ms:.6},\n",
            "  \"hottest_phase_p95_ms\": {hottest_phase_p95_ms:.6},\n",
            "  \"hottest_phase_max_ms\": {hottest_phase_max_ms:.6},\n",
            "  \"upload_budget_bytes_per_frame\": {upload_budget_bytes_per_frame},\n",
            "  \"terrain_view_buffer_updates_total\": {terrain_view_buffer_updates_total},\n",
            "  \"tile_tree_buffer_updates_total\": {tile_tree_buffer_updates_total},\n",
            "  \"tile_tree_buffer_skipped_total\": {tile_tree_buffer_skipped_total},\n",
            "  \"tile_requests_total\": {tile_requests_total},\n",
            "  \"tile_releases_total\": {tile_releases_total},\n",
            "  \"canceled_pending_attachment_loads_total\": {canceled_pending_attachment_loads_total},\n",
            "  \"canceled_inflight_attachment_loads_total\": {canceled_inflight_attachment_loads_total},\n",
            "  \"finished_attachment_loads_total\": {finished_attachment_loads_total},\n",
            "  \"upload_enqueued_attachment_tiles_total\": {upload_enqueued_attachment_tiles_total},\n",
            "  \"upload_enqueued_bytes_total\": {upload_enqueued_bytes_total},\n",
            "  \"upload_deferred_attachment_tiles_total\": {upload_deferred_attachment_tiles_total},\n",
            "  \"peak_pending_attachment_queue\": {peak_pending_attachment_queue},\n",
            "  \"peak_inflight_attachment_loads\": {peak_inflight_attachment_loads},\n",
            "  \"peak_upload_backlog_attachment_tiles\": {peak_upload_backlog_attachment_tiles},\n",
            "  \"canceled_stale_upload_attachment_tiles_total\": {canceled_stale_upload_attachment_tiles_total},\n",
            "  \"phase_timings\": {phase_timings_json}\n",
            "}}\n"
        ),
        scenario_name = scenario_name,
        overlays = overlays,
        present_mode = present_mode,
        focus_label = focus_label,
        focus_lat_deg = summary.focus_lat_deg,
        focus_lon_deg = summary.focus_lon_deg,
        benchmark_mode = summary.benchmark_mode,
        debug_tools_enabled = summary.debug_tools_enabled,
        perf_title_enabled = summary.perf_title_enabled,
        ready_wait_s = summary.ready_wait_s,
        ready_atlas_count = summary.ready_atlas_count,
        ready_loaded_atlas_count = summary.ready_loaded_atlas_count,
        ready_loaded_tile_total = summary.ready_loaded_tile_total,
        warmup_s = summary.warmup_s,
        duration_s = summary.duration_s,
        sample_count = summary.sample_count,
        fps_mean = summary.fps_mean,
        frame_ms_mean = summary.frame_ms_mean,
        frame_ms_min = summary.frame_ms_min,
        frame_ms_p50 = summary.frame_ms_p50,
        frame_ms_p90 = summary.frame_ms_p90,
        frame_ms_p95 = summary.frame_ms_p95,
        frame_ms_p99 = summary.frame_ms_p99,
        frame_ms_max = summary.frame_ms_max,
        frame_over_25ms_count = summary.frame_over_25ms_count,
        frame_over_33ms_count = summary.frame_over_33ms_count,
        frame_over_50ms_count = summary.frame_over_50ms_count,
        latency_estimate_ms = summary.latency_estimate_ms,
        peak_rss_kib = summary.peak_rss_kib,
        msaa_samples = summary.msaa_samples,
        benchmark_sweep_deg = summary.benchmark_sweep_deg,
        benchmark_sweep_period_s = summary.benchmark_sweep_period_s,
        drone_enabled = summary.drone_enabled,
        terrain_lighting_enabled = summary.terrain_lighting_enabled,
        terrain_morph_enabled = summary.terrain_morph_enabled,
        terrain_blend_enabled = summary.terrain_blend_enabled,
        terrain_sample_grad_enabled = summary.terrain_sample_grad_enabled,
        terrain_high_precision_enabled = summary.terrain_high_precision_enabled,
        hottest_phase_name = hottest_phase_name,
        hottest_phase_mean_ms = summary.hottest_phase_mean_ms,
        hottest_phase_p95_ms = summary.hottest_phase_p95_ms,
        hottest_phase_max_ms = summary.hottest_phase_max_ms,
        upload_budget_bytes_per_frame = summary.upload_budget_bytes_per_frame,
        terrain_view_buffer_updates_total = summary.terrain_view_buffer_updates_total,
        tile_tree_buffer_updates_total = summary.tile_tree_buffer_updates_total,
        tile_tree_buffer_skipped_total = summary.tile_tree_buffer_skipped_total,
        tile_requests_total = summary.tile_requests_total,
        tile_releases_total = summary.tile_releases_total,
        canceled_pending_attachment_loads_total = summary.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total = summary.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total = summary.finished_attachment_loads_total,
        upload_enqueued_attachment_tiles_total = summary.upload_enqueued_attachment_tiles_total,
        upload_enqueued_bytes_total = summary.upload_enqueued_bytes_total,
        upload_deferred_attachment_tiles_total = summary.upload_deferred_attachment_tiles_total,
        peak_pending_attachment_queue = summary.peak_pending_attachment_queue,
        peak_inflight_attachment_loads = summary.peak_inflight_attachment_loads,
        peak_upload_backlog_attachment_tiles = summary.peak_upload_backlog_attachment_tiles,
        canceled_stale_upload_attachment_tiles_total =
            summary.canceled_stale_upload_attachment_tiles_total,
        phase_timings_json = phase_timings_json,
    );
    fs::write(&json_path, json)?;

    let overlays_csv = overlays.replace(',', "+");
    let csv = format!(
        concat!(
            "scenario_name,overlays,present_mode,focus_label,focus_lat_deg,focus_lon_deg,benchmark_mode,debug_tools_enabled,perf_title_enabled,warmup_s,duration_s,sample_count,",
            "ready_wait_s,",
            "ready_atlas_count,ready_loaded_atlas_count,ready_loaded_tile_total,",
            "fps_mean,frame_ms_mean,frame_ms_min,frame_ms_p50,frame_ms_p90,frame_ms_p95,frame_ms_p99,frame_ms_max,frame_over_25ms_count,frame_over_33ms_count,frame_over_50ms_count,latency_estimate_ms,peak_rss_kib,msaa_samples,benchmark_sweep_deg,benchmark_sweep_period_s,drone_enabled,terrain_lighting_enabled,terrain_morph_enabled,terrain_blend_enabled,terrain_sample_grad_enabled,terrain_high_precision_enabled,",
            "hottest_phase_name,hottest_phase_mean_ms,hottest_phase_p95_ms,hottest_phase_max_ms,",
            "upload_budget_bytes_per_frame,terrain_view_buffer_updates_total,tile_tree_buffer_updates_total,tile_tree_buffer_skipped_total,",
            "tile_requests_total,tile_releases_total,canceled_pending_attachment_loads_total,canceled_inflight_attachment_loads_total,",
            "finished_attachment_loads_total,upload_enqueued_attachment_tiles_total,upload_enqueued_bytes_total,upload_deferred_attachment_tiles_total,",
            "peak_pending_attachment_queue,peak_inflight_attachment_loads,peak_upload_backlog_attachment_tiles,canceled_stale_upload_attachment_tiles_total\n",
            "\"{scenario_name}\",\"{overlays_csv}\",\"{present_mode}\",\"{focus_label}\",{focus_lat_deg:.6},{focus_lon_deg:.6},{benchmark_mode},{debug_tools_enabled},{perf_title_enabled},{warmup_s:.3},{duration_s:.3},{sample_count},{ready_wait_s:.3},",
            "{ready_atlas_count},{ready_loaded_atlas_count},{ready_loaded_tile_total},",
            "{fps_mean:.6},{frame_ms_mean:.6},{frame_ms_min:.6},{frame_ms_p50:.6},{frame_ms_p90:.6},{frame_ms_p95:.6},{frame_ms_p99:.6},{frame_ms_max:.6},{frame_over_25ms_count},{frame_over_33ms_count},{frame_over_50ms_count},{latency_estimate_ms:.6},{peak_rss_kib},{msaa_samples},{benchmark_sweep_deg:.3},{benchmark_sweep_period_s:.3},{drone_enabled},{terrain_lighting_enabled},{terrain_morph_enabled},{terrain_blend_enabled},{terrain_sample_grad_enabled},{terrain_high_precision_enabled},\"{hottest_phase_name}\",{hottest_phase_mean_ms:.6},{hottest_phase_p95_ms:.6},{hottest_phase_max_ms:.6},",
            "{upload_budget_bytes_per_frame},{terrain_view_buffer_updates_total},{tile_tree_buffer_updates_total},{tile_tree_buffer_skipped_total},",
            "{tile_requests_total},{tile_releases_total},{canceled_pending_attachment_loads_total},{canceled_inflight_attachment_loads_total},",
            "{finished_attachment_loads_total},{upload_enqueued_attachment_tiles_total},{upload_enqueued_bytes_total},{upload_deferred_attachment_tiles_total},",
            "{peak_pending_attachment_queue},{peak_inflight_attachment_loads},{peak_upload_backlog_attachment_tiles},{canceled_stale_upload_attachment_tiles_total}\n"
        ),
        scenario_name = scenario_name,
        overlays_csv = overlays_csv,
        present_mode = present_mode,
        focus_label = focus_label,
        focus_lat_deg = summary.focus_lat_deg,
        focus_lon_deg = summary.focus_lon_deg,
        benchmark_mode = summary.benchmark_mode,
        debug_tools_enabled = summary.debug_tools_enabled,
        perf_title_enabled = summary.perf_title_enabled,
        warmup_s = summary.warmup_s,
        duration_s = summary.duration_s,
        sample_count = summary.sample_count,
        ready_wait_s = summary.ready_wait_s,
        ready_atlas_count = summary.ready_atlas_count,
        ready_loaded_atlas_count = summary.ready_loaded_atlas_count,
        ready_loaded_tile_total = summary.ready_loaded_tile_total,
        fps_mean = summary.fps_mean,
        frame_ms_mean = summary.frame_ms_mean,
        frame_ms_min = summary.frame_ms_min,
        frame_ms_p50 = summary.frame_ms_p50,
        frame_ms_p90 = summary.frame_ms_p90,
        frame_ms_p95 = summary.frame_ms_p95,
        frame_ms_p99 = summary.frame_ms_p99,
        frame_ms_max = summary.frame_ms_max,
        frame_over_25ms_count = summary.frame_over_25ms_count,
        frame_over_33ms_count = summary.frame_over_33ms_count,
        frame_over_50ms_count = summary.frame_over_50ms_count,
        latency_estimate_ms = summary.latency_estimate_ms,
        peak_rss_kib = summary.peak_rss_kib,
        msaa_samples = summary.msaa_samples,
        benchmark_sweep_deg = summary.benchmark_sweep_deg,
        benchmark_sweep_period_s = summary.benchmark_sweep_period_s,
        drone_enabled = summary.drone_enabled,
        terrain_lighting_enabled = summary.terrain_lighting_enabled,
        terrain_morph_enabled = summary.terrain_morph_enabled,
        terrain_blend_enabled = summary.terrain_blend_enabled,
        terrain_sample_grad_enabled = summary.terrain_sample_grad_enabled,
        terrain_high_precision_enabled = summary.terrain_high_precision_enabled,
        hottest_phase_name = hottest_phase_name,
        hottest_phase_mean_ms = summary.hottest_phase_mean_ms,
        hottest_phase_p95_ms = summary.hottest_phase_p95_ms,
        hottest_phase_max_ms = summary.hottest_phase_max_ms,
        upload_budget_bytes_per_frame = summary.upload_budget_bytes_per_frame,
        terrain_view_buffer_updates_total = summary.terrain_view_buffer_updates_total,
        tile_tree_buffer_updates_total = summary.tile_tree_buffer_updates_total,
        tile_tree_buffer_skipped_total = summary.tile_tree_buffer_skipped_total,
        tile_requests_total = summary.tile_requests_total,
        tile_releases_total = summary.tile_releases_total,
        canceled_pending_attachment_loads_total = summary.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total = summary.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total = summary.finished_attachment_loads_total,
        upload_enqueued_attachment_tiles_total = summary.upload_enqueued_attachment_tiles_total,
        upload_enqueued_bytes_total = summary.upload_enqueued_bytes_total,
        upload_deferred_attachment_tiles_total = summary.upload_deferred_attachment_tiles_total,
        peak_pending_attachment_queue = summary.peak_pending_attachment_queue,
        peak_inflight_attachment_loads = summary.peak_inflight_attachment_loads,
        peak_upload_backlog_attachment_tiles = summary.peak_upload_backlog_attachment_tiles,
        canceled_stale_upload_attachment_tiles_total =
            summary.canceled_stale_upload_attachment_tiles_total,
    );
    fs::write(&csv_path, csv)?;

    Ok((json_path, csv_path))
}

fn run_benchmark(
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
    config: Option<Res<BenchmarkConfig>>,
    focus: Option<Res<CameraFocus>>,
    mode: Res<RuntimeMode>,
    debug: Res<DebugTerrain>,
    settings: Res<TerrainSettings>,
    perf_telemetry: Res<TerrainPerfTelemetry>,
    mut tile_trees: ResMut<TerrainViewComponents<TileTree>>,
    mut tile_atlases: Query<&mut TileAtlas>,
    mut runtime: Local<BenchmarkRuntime>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let Some(config) = config else {
        return;
    };

    if runtime.completed {
        return;
    }

    let delta_s = time.delta_secs_f64();
    runtime.status_log_elapsed_s += delta_s;

    let mut atlas_count = 0usize;
    let mut loaded_atlas_count = 0usize;
    let mut loaded_tile_total = 0usize;
    for tile_atlas in &tile_atlases {
        atlas_count += 1;
        let loaded = tile_atlas.loaded_tile_count();
        loaded_tile_total += loaded;
        if loaded > 0 {
            loaded_atlas_count += 1;
        }
    }

    let terrain_ready =
        atlas_count > 0 && loaded_atlas_count == atlas_count && loaded_tile_total > 0;
    if !terrain_ready {
        runtime.ready_wait_s += delta_s;

        if runtime.status_log_elapsed_s >= 1.0 {
            runtime.status_log_elapsed_s = 0.0;
            info!(
                target: "perf",
                "benchmark waiting_for_terrain ready_atlases={}/{} loaded_tiles={} waited_s={:.2}",
                loaded_atlas_count,
                atlas_count,
                loaded_tile_total,
                runtime.ready_wait_s
            );
        }

        if runtime.ready_wait_s >= config.ready_timeout_s {
            error!(
                target: "perf",
                "benchmark timed out waiting for terrain after {:.2}s (ready_atlases={}/{} loaded_tiles={})",
                runtime.ready_wait_s,
                loaded_atlas_count,
                atlas_count,
                loaded_tile_total
            );
            runtime.completed = true;
            app_exit.write(AppExit::error());
        }
        return;
    }

    if !runtime.saw_ready_once {
        runtime.saw_ready_once = true;
        runtime.ready_atlas_count = atlas_count;
        runtime.ready_loaded_atlas_count = loaded_atlas_count;
        runtime.ready_loaded_tile_total = loaded_tile_total;
        runtime.status_log_elapsed_s = 0.0;
        info!(
            target: "perf",
            "benchmark terrain_ready after {:.2}s (ready_atlases={}/{} loaded_tiles={}); starting warmup {:.2}s",
            runtime.ready_wait_s,
            loaded_atlas_count,
            atlas_count,
            loaded_tile_total,
            config.warmup_s
        );
    }

    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed())
        .unwrap_or(time.delta_secs_f64() * 1000.0);

    if !runtime.measurement_window_started && runtime.warmup_elapsed_s < config.warmup_s {
        runtime.warmup_elapsed_s += delta_s;
        if runtime.warmup_elapsed_s >= config.warmup_s {
            perf_telemetry.reset();
            for tile_tree in tile_trees.values_mut() {
                tile_tree.reset_perf_counters();
            }
            for mut tile_atlas in &mut tile_atlases {
                tile_atlas.reset_perf_counters();
            }
            runtime.measurement_window_started = true;
            info!(
                target: "perf",
                "benchmark warmup_complete waited_s={:.2} warmup_s={:.2}; starting measurement {:.2}s",
                runtime.ready_wait_s,
                config.warmup_s,
                config.duration_s
            );
        }
        return;
    }

    if !runtime.measurement_window_started {
        perf_telemetry.reset();
        for tile_tree in tile_trees.values_mut() {
            tile_tree.reset_perf_counters();
        }
        for mut tile_atlas in &mut tile_atlases {
            tile_atlas.reset_perf_counters();
        }
        runtime.measurement_window_started = true;
        info!(
            target: "perf",
            "benchmark warmup_complete waited_s={:.2} warmup_s={:.2}; starting measurement {:.2}s",
            runtime.ready_wait_s,
            config.warmup_s,
            config.duration_s
        );
    }

    runtime.measure_elapsed_s += delta_s;
    runtime.samples_ms.push(frame_ms);

    if runtime.measure_elapsed_s < config.duration_s {
        return;
    }

    let summary = match compute_summary(
        &config,
        &runtime.samples_ms,
        runtime.ready_wait_s,
        focus.as_deref(),
        &mode,
        &debug,
        &runtime,
        &settings,
        perf_telemetry.snapshot(),
        tile_trees
            .values()
            .map(|tile_tree| tile_tree.perf_counters()),
        tile_atlases
            .iter()
            .map(|tile_atlas| tile_atlas.perf_counters()),
    ) {
        Some(summary) => summary,
        None => {
            error!(
                target: "perf",
                "benchmark produced no samples: output={}",
                config.output_path.display()
            );
            runtime.completed = true;
            app_exit.write(AppExit::error());
            return;
        }
    };

    match write_benchmark_outputs(&config.output_path, &summary) {
        Ok((json_path, csv_path)) => {
            info!(
                target: "perf",
                "benchmark complete: scenario={} samples={} fps_mean={:.2} frame_ms_mean={:.3} p95_ms={:.3} p99_ms={:.3} latency_est_ms={:.3} peak_rss_kib={} hottest_phase={} hottest_phase_p95_ms={:.3} ready_atlases={}/{} loaded_tiles={} uploads={} deferred_uploads={} json={} csv={}",
                summary.scenario_name,
                summary.sample_count,
                summary.fps_mean,
                summary.frame_ms_mean,
                summary.frame_ms_p95,
                summary.frame_ms_p99,
                summary.latency_estimate_ms,
                summary.peak_rss_kib,
                summary.hottest_phase_name,
                summary.hottest_phase_p95_ms,
                summary.ready_loaded_atlas_count,
                summary.ready_atlas_count,
                summary.ready_loaded_tile_total,
                summary.upload_enqueued_attachment_tiles_total,
                summary.upload_deferred_attachment_tiles_total,
                json_path.display(),
                csv_path.display()
            );
            runtime.completed = true;
            app_exit.write(AppExit::Success);
        }
        Err(error) => {
            error!(
                target: "perf",
                "failed to write benchmark output at {}: {error}",
                config.output_path.display()
            );
            runtime.completed = true;
            app_exit.write(AppExit::error());
        }
    }
}
