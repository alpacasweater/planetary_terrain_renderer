use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::shader::ShaderRef;
use bevy::time::Real;
use bevy::window::{PresentMode, WindowResolution};
use bevy::{prelude::*, reflect::TypePath, render::render_resource::*};
use big_space::prelude::{CellCoord, Grids};
use bevy_terrain::math::geodesy::unit_from_lat_lon_degrees;
use bevy_terrain::prelude::*;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
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
const BENCHMARK_SWEEP_DEG_ENV: &str = "MULTIRES_BENCHMARK_SWEEP_DEG";
const BENCHMARK_SWEEP_PERIOD_ENV: &str = "MULTIRES_BENCHMARK_SWEEP_PERIOD_SECONDS";
const CAPTURE_DIR_ENV: &str = "MULTIRES_CAPTURE_DIR";
const CAPTURE_FRAMES_ENV: &str = "MULTIRES_CAPTURE_FRAMES";
const PERF_TITLE_PREFIX: &str = "SphericalMultires";

#[derive(Clone, Copy)]
struct OverlayPreset {
    config_path: &'static str,
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
    saw_ready_once: bool,
    samples_ms: Vec<f64>,
    completed: bool,
}

struct BenchmarkSummary {
    focus_label: String,
    focus_lat_deg: f64,
    focus_lon_deg: f64,
    ready_wait_s: f64,
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
    latency_estimate_ms: f64,
}

#[derive(Component)]
struct PrimaryTerrainCamera;

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
        TerrainDebugPlugin,
        TerrainPickingPlugin,
        FrameTimeDiagnosticsPlugin::default(),
    ));

    app.insert_resource(TerrainSettings::new(vec!["albedo"]))
        .insert_resource(PerfTitleState::default())
        .add_systems(Startup, initialize)
        .add_systems(
            Update,
            (
                animate_benchmark_camera,
                capture_benchmark_frames,
                update_perf_title,
                run_benchmark,
            ),
        )
        .run();
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
                label: "Swiss Alps",
                focus_lat_deg: 46.8,
                focus_lon_deg: 8.2,
            },
        ),
        (
            "saxony",
            OverlayPreset {
                config_path: "terrains/saxony_partial/config.tc.ron",
                label: "Saxony",
                focus_lat_deg: 50.9,
                focus_lon_deg: 13.5,
            },
        ),
        (
            "los",
            OverlayPreset {
                config_path: "terrains/los_highres/config.tc.ron",
                label: "Los Angeles",
                focus_lat_deg: 34.05,
                focus_lon_deg: -118.25,
            },
        ),
        (
            "srtm_n27e086",
            OverlayPreset {
                config_path: "terrains/srtm_n27e086/config.tc.ron",
                label: "Himalaya",
                focus_lat_deg: 27.9,
                focus_lon_deg: 86.9,
            },
        ),
        (
            "srtm_n35e139",
            OverlayPreset {
                config_path: "terrains/srtm_n35e139/config.tc.ron",
                label: "Tokyo",
                focus_lat_deg: 35.68,
                focus_lon_deg: 139.76,
            },
        ),
        (
            "srtm_n37e127",
            OverlayPreset {
                config_path: "terrains/srtm_n37e127/config.tc.ron",
                label: "Korea",
                focus_lat_deg: 37.57,
                focus_lon_deg: 126.98,
            },
        ),
        (
            "srtm_n39w077",
            OverlayPreset {
                config_path: "terrains/srtm_n39w077/config.tc.ron",
                label: "DC Region",
                focus_lat_deg: 39.0,
                focus_lon_deg: -77.0,
            },
        ),
        (
            "srtm_n51e000",
            OverlayPreset {
                config_path: "terrains/srtm_n51e000/config.tc.ron",
                label: "London",
                focus_lat_deg: 51.5,
                focus_lon_deg: 0.0,
            },
        ),
        (
            "srtm_s22w043",
            OverlayPreset {
                config_path: "terrains/srtm_s22w043/config.tc.ron",
                label: "Rio",
                focus_lat_deg: -22.9,
                focus_lon_deg: -43.2,
            },
        ),
        (
            "srtm_s33e151",
            OverlayPreset {
                config_path: "terrains/srtm_s33e151/config.tc.ron",
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

    Some(BenchmarkConfig {
        output_path: PathBuf::from(output),
        warmup_s,
        duration_s,
        ready_timeout_s,
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
) {
    let overlay_map = overlay_config_map();
    let selected_keys = selected_overlay_keys();

    let benchmark_config = benchmark_config_from_env();
    let benchmark_enabled = benchmark_config.is_some();
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

    let mut view = Entity::PLACEHOLDER;
    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                camera_transform,
                PrimaryTerrainCamera,
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

fn compute_summary(
    config: &BenchmarkConfig,
    samples_ms: &[f64],
    ready_wait_s: f64,
    focus: Option<&CameraFocus>,
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

    Some(BenchmarkSummary {
        focus_label,
        focus_lat_deg,
        focus_lon_deg,
        ready_wait_s,
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
        latency_estimate_ms: frame_ms_p95,
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

    let focus_label = summary.focus_label.replace('"', "'");
    let json = format!(
        concat!(
            "{{\n",
            "  \"focus_label\": \"{focus_label}\",\n",
            "  \"focus_lat_deg\": {focus_lat_deg:.6},\n",
            "  \"focus_lon_deg\": {focus_lon_deg:.6},\n",
            "  \"ready_wait_s\": {ready_wait_s:.3},\n",
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
            "  \"latency_estimate_ms\": {latency_estimate_ms:.6}\n",
            "}}\n"
        ),
        focus_label = focus_label,
        focus_lat_deg = summary.focus_lat_deg,
        focus_lon_deg = summary.focus_lon_deg,
        ready_wait_s = summary.ready_wait_s,
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
        latency_estimate_ms = summary.latency_estimate_ms,
    );
    fs::write(&json_path, json)?;

    let csv = format!(
        concat!(
            "focus_label,focus_lat_deg,focus_lon_deg,warmup_s,duration_s,sample_count,",
            "ready_wait_s,",
            "fps_mean,frame_ms_mean,frame_ms_min,frame_ms_p50,frame_ms_p90,frame_ms_p95,frame_ms_p99,frame_ms_max,latency_estimate_ms\n",
            "\"{focus_label}\",{focus_lat_deg:.6},{focus_lon_deg:.6},{warmup_s:.3},{duration_s:.3},{sample_count},{ready_wait_s:.3},",
            "{fps_mean:.6},{frame_ms_mean:.6},{frame_ms_min:.6},{frame_ms_p50:.6},{frame_ms_p90:.6},{frame_ms_p95:.6},{frame_ms_p99:.6},{frame_ms_max:.6},{latency_estimate_ms:.6}\n"
        ),
        focus_label = focus_label,
        focus_lat_deg = summary.focus_lat_deg,
        focus_lon_deg = summary.focus_lon_deg,
        warmup_s = summary.warmup_s,
        duration_s = summary.duration_s,
        sample_count = summary.sample_count,
        ready_wait_s = summary.ready_wait_s,
        fps_mean = summary.fps_mean,
        frame_ms_mean = summary.frame_ms_mean,
        frame_ms_min = summary.frame_ms_min,
        frame_ms_p50 = summary.frame_ms_p50,
        frame_ms_p90 = summary.frame_ms_p90,
        frame_ms_p95 = summary.frame_ms_p95,
        frame_ms_p99 = summary.frame_ms_p99,
        frame_ms_max = summary.frame_ms_max,
        latency_estimate_ms = summary.latency_estimate_ms,
    );
    fs::write(&csv_path, csv)?;

    Ok((json_path, csv_path))
}

fn run_benchmark(
    diagnostics: Res<DiagnosticsStore>,
    time: Res<Time<Real>>,
    config: Option<Res<BenchmarkConfig>>,
    focus: Option<Res<CameraFocus>>,
    tile_atlases: Query<&TileAtlas>,
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

    if runtime.warmup_elapsed_s < config.warmup_s {
        runtime.warmup_elapsed_s += delta_s;
        if runtime.warmup_elapsed_s >= config.warmup_s {
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
                "benchmark complete: samples={} fps_mean={:.2} frame_ms_mean={:.3} p95_ms={:.3} latency_est_ms={:.3} json={} csv={}",
                summary.sample_count,
                summary.fps_mean,
                summary.frame_ms_mean,
                summary.frame_ms_p95,
                summary.latency_estimate_ms,
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
