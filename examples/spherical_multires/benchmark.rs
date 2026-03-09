use super::demo::{benchmark_mode_enabled, env_bool, env_f64, env_u32};
// Benchmark, capture, and perf-reporting support for the multires demo.

use super::*;

#[cfg(feature = "metal_capture")]
pub fn schedule_metal_capture(
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

pub fn capture_benchmark_frames(
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

pub fn update_perf_title(
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
        perf.started_attachment_loads_total += counters.started_attachment_loads_total;
        perf.cache_resolved_attachment_loads_total +=
            counters.cache_resolved_attachment_loads_total;
        perf.starter_resolved_attachment_loads_total +=
            counters.starter_resolved_attachment_loads_total;
        perf.canceled_pending_attachment_loads_total +=
            counters.canceled_pending_attachment_loads_total;
        perf.canceled_inflight_attachment_loads_total +=
            counters.canceled_inflight_attachment_loads_total;
        perf.canceled_stale_upload_attachment_tiles_total +=
            counters.canceled_stale_upload_attachment_tiles_total;
        perf.finished_attachment_loads_total += counters.finished_attachment_loads_total;
        perf.failed_attachment_loads_total += counters.failed_attachment_loads_total;
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
        started_attachment_loads_total: perf.started_attachment_loads_total,
        cache_resolved_attachment_loads_total: perf.cache_resolved_attachment_loads_total,
        starter_resolved_attachment_loads_total: perf.starter_resolved_attachment_loads_total,
        canceled_pending_attachment_loads_total: perf.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total: perf.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total: perf.finished_attachment_loads_total,
        failed_attachment_loads_total: perf.failed_attachment_loads_total,
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
            "  \"started_attachment_loads_total\": {started_attachment_loads_total},\n",
            "  \"cache_resolved_attachment_loads_total\": {cache_resolved_attachment_loads_total},\n",
            "  \"starter_resolved_attachment_loads_total\": {starter_resolved_attachment_loads_total},\n",
            "  \"canceled_pending_attachment_loads_total\": {canceled_pending_attachment_loads_total},\n",
            "  \"canceled_inflight_attachment_loads_total\": {canceled_inflight_attachment_loads_total},\n",
            "  \"finished_attachment_loads_total\": {finished_attachment_loads_total},\n",
            "  \"failed_attachment_loads_total\": {failed_attachment_loads_total},\n",
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
        started_attachment_loads_total = summary.started_attachment_loads_total,
        cache_resolved_attachment_loads_total = summary.cache_resolved_attachment_loads_total,
        starter_resolved_attachment_loads_total = summary.starter_resolved_attachment_loads_total,
        canceled_pending_attachment_loads_total = summary.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total = summary.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total = summary.finished_attachment_loads_total,
        failed_attachment_loads_total = summary.failed_attachment_loads_total,
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
            "tile_requests_total,tile_releases_total,started_attachment_loads_total,cache_resolved_attachment_loads_total,starter_resolved_attachment_loads_total,",
            "canceled_pending_attachment_loads_total,canceled_inflight_attachment_loads_total,finished_attachment_loads_total,failed_attachment_loads_total,",
            "upload_enqueued_attachment_tiles_total,upload_enqueued_bytes_total,upload_deferred_attachment_tiles_total,",
            "peak_pending_attachment_queue,peak_inflight_attachment_loads,peak_upload_backlog_attachment_tiles,canceled_stale_upload_attachment_tiles_total\n",
            "\"{scenario_name}\",\"{overlays_csv}\",\"{present_mode}\",\"{focus_label}\",{focus_lat_deg:.6},{focus_lon_deg:.6},{benchmark_mode},{debug_tools_enabled},{perf_title_enabled},{warmup_s:.3},{duration_s:.3},{sample_count},{ready_wait_s:.3},",
            "{ready_atlas_count},{ready_loaded_atlas_count},{ready_loaded_tile_total},",
            "{fps_mean:.6},{frame_ms_mean:.6},{frame_ms_min:.6},{frame_ms_p50:.6},{frame_ms_p90:.6},{frame_ms_p95:.6},{frame_ms_p99:.6},{frame_ms_max:.6},{frame_over_25ms_count},{frame_over_33ms_count},{frame_over_50ms_count},{latency_estimate_ms:.6},{peak_rss_kib},{msaa_samples},{benchmark_sweep_deg:.3},{benchmark_sweep_period_s:.3},{drone_enabled},{terrain_lighting_enabled},{terrain_morph_enabled},{terrain_blend_enabled},{terrain_sample_grad_enabled},{terrain_high_precision_enabled},\"{hottest_phase_name}\",{hottest_phase_mean_ms:.6},{hottest_phase_p95_ms:.6},{hottest_phase_max_ms:.6},",
            "{upload_budget_bytes_per_frame},{terrain_view_buffer_updates_total},{tile_tree_buffer_updates_total},{tile_tree_buffer_skipped_total},",
            "{tile_requests_total},{tile_releases_total},{started_attachment_loads_total},{cache_resolved_attachment_loads_total},{starter_resolved_attachment_loads_total},",
            "{canceled_pending_attachment_loads_total},{canceled_inflight_attachment_loads_total},{finished_attachment_loads_total},{failed_attachment_loads_total},",
            "{upload_enqueued_attachment_tiles_total},{upload_enqueued_bytes_total},{upload_deferred_attachment_tiles_total},",
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
        started_attachment_loads_total = summary.started_attachment_loads_total,
        cache_resolved_attachment_loads_total = summary.cache_resolved_attachment_loads_total,
        starter_resolved_attachment_loads_total = summary.starter_resolved_attachment_loads_total,
        canceled_pending_attachment_loads_total = summary.canceled_pending_attachment_loads_total,
        canceled_inflight_attachment_loads_total = summary.canceled_inflight_attachment_loads_total,
        finished_attachment_loads_total = summary.finished_attachment_loads_total,
        failed_attachment_loads_total = summary.failed_attachment_loads_total,
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

pub fn run_benchmark(
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
