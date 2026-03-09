//! Multi-resolution planetary terrain demo with optional overlays, benchmarking, and click inspection.

use bevy::app::AppExit;
#[cfg(feature = "metal_capture")]
use bevy::diagnostic::FrameCount;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::input::ButtonInput;
use bevy::render::RenderApp;
use bevy::render::view::Msaa;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::time::Real;
use bevy::window::{PresentMode, WindowResolution};
use bevy::{math::DVec3, prelude::*};
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

#[path = "spherical_multires/benchmark.rs"]
mod benchmark;
#[path = "spherical_multires/click.rs"]
mod click;
#[path = "spherical_multires/demo.rs"]
mod demo;

use benchmark::{capture_benchmark_frames, run_benchmark, update_perf_title};
use click::{
    copy_click_readout_to_clipboard, inspect_clicked_terrain_point, update_click_readout_ui,
};
#[cfg(feature = "metal_capture")]
use demo::metal_capture_config_from_env;
use demo::{
    animate_benchmark_camera, animate_demo_drone, benchmark_mode_enabled, env_bool, env_usize,
    finish_loading_images_local, initialize, present_mode_from_env, terrain_debug_from_env,
};

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
        SimpleTerrainMaterialPlugin,
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
        TerrainSettings::with_albedo()
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
            .add_systems(Update, benchmark::schedule_metal_capture);
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

fn asset_dir_exists(asset_path: &str) -> bool {
    let fs_path = format!("assets/{asset_path}");
    Path::new(&fs_path).is_dir()
}
