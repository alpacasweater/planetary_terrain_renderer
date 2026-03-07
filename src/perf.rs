use bevy::prelude::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct TerrainPhaseTimingSummary {
    pub name: String,
    pub sample_count: usize,
    pub mean_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, Default)]
pub struct TerrainPerfSnapshot {
    pub phase_timings: Vec<TerrainPhaseTimingSummary>,
}

impl TerrainPerfSnapshot {
    pub fn hottest_by_p95(&self) -> Option<&TerrainPhaseTimingSummary> {
        self.phase_timings.iter().max_by(|a, b| {
            a.p95_ms
                .partial_cmp(&b.p95_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

#[derive(Clone, Resource, Default)]
pub struct TerrainPerfTelemetry {
    enabled: Arc<AtomicBool>,
    inner: Arc<Mutex<TerrainPerfTelemetryInner>>,
}

#[derive(Default)]
struct TerrainPerfTelemetryInner {
    samples_ms_by_phase: BTreeMap<&'static str, Vec<f64>>,
}

impl TerrainPerfTelemetry {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn reset(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.samples_ms_by_phase.clear();
        }
    }

    pub fn record_duration(&self, phase: &'static str, duration: Duration) {
        if !self.is_enabled() {
            return;
        }
        if let Ok(mut inner) = self.inner.lock() {
            inner
                .samples_ms_by_phase
                .entry(phase)
                .or_default()
                .push(duration.as_secs_f64() * 1000.0);
        }
    }

    pub fn snapshot(&self) -> TerrainPerfSnapshot {
        let Ok(inner) = self.inner.lock() else {
            return TerrainPerfSnapshot::default();
        };

        let mut phase_timings = Vec::with_capacity(inner.samples_ms_by_phase.len());
        for (&name, samples) in &inner.samples_ms_by_phase {
            if samples.is_empty() {
                continue;
            }

            let mut sorted = samples.clone();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let sample_count = sorted.len();
            let mean_ms = sorted.iter().sum::<f64>() / sample_count as f64;
            let p95_ms = percentile(&sorted, 0.95);
            let p99_ms = percentile(&sorted, 0.99);
            let max_ms = *sorted.last().unwrap_or(&0.0);

            phase_timings.push(TerrainPhaseTimingSummary {
                name: name.to_string(),
                sample_count,
                mean_ms,
                p95_ms,
                p99_ms,
                max_ms,
            });
        }

        phase_timings.sort_by(|a, b| a.name.cmp(&b.name));

        TerrainPerfSnapshot { phase_timings }
    }
}

fn percentile(sorted_samples: &[f64], percentile: f64) -> f64 {
    if sorted_samples.is_empty() {
        return 0.0;
    }
    let index =
        ((sorted_samples.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).round() as usize;
    sorted_samples[index]
}

pub const PHASE_MAIN_TILE_ATLAS_UPDATE: &str = "main.tile_atlas_update";
pub const PHASE_MAIN_UPDATE_TERRAIN_BUFFER: &str = "main.update_terrain_buffer";
pub const PHASE_MAIN_UPDATE_TERRAIN_VIEW_BUFFER: &str = "main.update_terrain_view_buffer";
pub const PHASE_RENDER_EXTRACT_GPU_TILE_ATLAS: &str = "render.extract.gpu_tile_atlas";
pub const PHASE_RENDER_PREPARE_GPU_TILE_ATLAS: &str = "render.prepare.gpu_tile_atlas";
pub const PHASE_RENDER_PREPARE_GPU_TILE_ATLAS_MIP_BIND_GROUPS: &str =
    "render.prepare.gpu_tile_atlas.mip_bind_groups";
pub const PHASE_RENDER_PREPARE_GPU_TILE_ATLAS_UPLOADS: &str =
    "render.prepare.gpu_tile_atlas.uploads";
pub const PHASE_RENDER_PREPARE_GPU_TERRAIN: &str = "render.prepare.gpu_terrain";
pub const PHASE_RENDER_PREPARE_TERRAIN_VIEW: &str = "render.prepare.terrain_view_bind_group";
pub const PHASE_RENDER_PREPARE_INDIRECT: &str = "render.prepare.indirect_bind_group";
pub const PHASE_RENDER_PREPARE_REFINE_TILES: &str = "render.prepare.refine_tiles_bind_group";
pub const PHASE_RENDER_PREPARE_DEPTH_TEXTURES: &str =
    "render.prepare_resources.terrain_depth_textures";
pub const PHASE_RENDER_QUEUE_TILING_PREPASS: &str = "render.queue.tiling_prepass";
pub const PHASE_RENDER_QUEUE_GPU_TILE_ATLAS: &str = "render.queue.gpu_tile_atlas";
pub const PHASE_RENDER_NODE_MIP_PREPASS_CPU: &str = "render.node.mip_prepass_cpu";
pub const PHASE_RENDER_NODE_TILING_PREPASS_CPU: &str = "render.node.tiling_prepass_cpu";
pub const PHASE_RENDER_NODE_TERRAIN_PASS_CPU: &str = "render.node.terrain_pass_cpu";
