use crate::{
    plugin::TerrainSettings,
    streaming::{
        CacheFirstLocalTileSource, CacheTileEncoding, CachedTileMetadata, LocalTileRequest,
        LocalTileSourceKind, NasaGibsImageryProvider, OpenTopographyHeightProvider,
        StreamedAttachmentKind, StreamingProviderError, StreamingSourceDescriptor,
        StreamingSourceKind, StreamingTileProvider,
        cache_writer::{StreamingCacheWriteError, write_materialized_tile},
        source_contract::StreamingTileRequest,
    },
    terrain_data::{AttachmentConfig, AttachmentFormat, AttachmentLabel, TileAtlas},
};
use bevy::{
    platform::collections::{HashMap, HashSet},
    prelude::*,
    tasks::{IoTaskPool, Task, poll_once},
};
use std::cmp::Reverse;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tiff::{
    decoder::{Decoder, DecodingResult},
    encoder::{TiffEncoder, colortype},
};

#[derive(Clone, Debug, PartialEq, Eq, Resource)]
pub struct TerrainStreamingSettings {
    pub offline_only: bool,
    pub stream_imagery: bool,
    pub stream_height: bool,
    pub max_pending_requests: usize,
    pub max_inflight_requests: usize,
}

impl Default for TerrainStreamingSettings {
    fn default() -> Self {
        Self {
            offline_only: true,
            stream_imagery: true,
            stream_height: false,
            max_pending_requests: 1024,
            max_inflight_requests: 4,
        }
    }
}

impl TerrainStreamingSettings {
    pub fn online_imagery() -> Self {
        Self {
            offline_only: false,
            stream_imagery: true,
            stream_height: false,
            max_pending_requests: 1024,
            max_inflight_requests: 4,
        }
    }

    pub fn online_imagery_and_height() -> Self {
        Self {
            offline_only: false,
            stream_imagery: true,
            stream_height: true,
            max_pending_requests: 1024,
            max_inflight_requests: 4,
        }
    }

    pub fn with_max_pending_requests(mut self, max_pending_requests: usize) -> Self {
        self.max_pending_requests = max_pending_requests;
        self
    }

    pub fn with_max_inflight_requests(mut self, max_inflight_requests: usize) -> Self {
        self.max_inflight_requests = max_inflight_requests;
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamingQueueStats {
    pub enqueued_total: u64,
    pub deduped_total: u64,
    pub dropped_offline_total: u64,
    pub dropped_policy_total: u64,
    pub dropped_capacity_total: u64,
    pub completed_total: u64,
    pub failed_total: u64,
}

#[derive(Clone, Debug)]
pub struct QueuedStreamingRequest {
    pub request: StreamingTileRequest,
    pub sequence: u64,
    pub priority_class: u8,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct StreamingRequestKey {
    terrain_path: String,
    attachment_label: AttachmentLabel,
    coordinate: crate::math::TileCoordinate,
}

impl StreamingRequestKey {
    fn from_request(request: &StreamingTileRequest) -> Self {
        Self {
            terrain_path: request.terrain_path.clone(),
            attachment_label: request.attachment_label.clone(),
            coordinate: request.coordinate,
        }
    }
}

#[derive(Resource, Default)]
pub struct StreamingRequestQueue {
    pending: HashMap<StreamingRequestKey, QueuedStreamingRequest>,
    inflight: HashSet<StreamingRequestKey>,
    next_sequence: u64,
    stats: StreamingQueueStats,
}

fn request_priority(
    priority_class: u8,
    request: &StreamingTileRequest,
    sequence: u64,
) -> (u8, u8, u32, u64) {
    (
        request.priority as u8,
        priority_class,
        request.coordinate.lod,
        sequence,
    )
}

fn attachment_priority(attachment_label: &AttachmentLabel) -> u8 {
    match attachment_label {
        AttachmentLabel::Custom(name) if name == "albedo" => 1,
        AttachmentLabel::Height => 0,
        AttachmentLabel::Custom(_) | AttachmentLabel::Empty(_) => 0,
    }
}

impl StreamingRequestQueue {
    pub fn stats(&self) -> &StreamingQueueStats {
        &self.stats
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn inflight_count(&self) -> usize {
        self.inflight.len()
    }

    pub fn enqueue(
        &mut self,
        request: StreamingTileRequest,
        settings: &TerrainStreamingSettings,
    ) -> bool {
        if settings.offline_only {
            self.stats.dropped_offline_total += 1;
            return false;
        }

        if !streaming_allowed_for(&request.attachment_label, settings) {
            self.stats.dropped_policy_total += 1;
            return false;
        }

        let key = StreamingRequestKey::from_request(&request);
        if self.pending.contains_key(&key) || self.inflight.contains(&key) {
            self.stats.deduped_total += 1;
            return false;
        }

        let sequence = self.next_sequence;
        let priority_class = attachment_priority(&request.attachment_label);
        let priority = request_priority(priority_class, &request, sequence);

        if self.pending.len() >= settings.max_pending_requests {
            let worst_pending = self
                .pending
                .iter()
                .map(|(key, queued)| {
                    (
                        key.clone(),
                        request_priority(queued.priority_class, &queued.request, queued.sequence),
                    )
                })
                .min_by_key(|(_, priority)| *priority);

            if let Some((worst_key, worst_priority)) = worst_pending {
                if priority > worst_priority {
                    self.pending.remove(&worst_key);
                } else {
                    self.stats.dropped_capacity_total += 1;
                    return false;
                }
            }
        }

        self.next_sequence += 1;
        self.pending.insert(
            key,
            QueuedStreamingRequest {
                request,
                sequence,
                priority_class,
            },
        );
        self.stats.enqueued_total += 1;
        true
    }

    pub fn dequeue_batch(&mut self, limit: usize) -> Vec<QueuedStreamingRequest> {
        let mut queued = self
            .pending
            .drain()
            .map(|(key, request)| (key, request))
            .collect::<Vec<_>>();
        queued.sort_by_key(|(_, request)| {
            Reverse(request_priority(
                request.priority_class,
                &request.request,
                request.sequence,
            ))
        });

        let take_count = limit.min(queued.len());
        let mut drained = Vec::with_capacity(take_count);

        for (index, (key, request)) in queued.into_iter().enumerate() {
            if index < take_count {
                self.inflight.insert(key);
                drained.push(request);
            } else {
                self.pending.insert(key, request);
            }
        }

        drained
    }

    pub fn finish(&mut self, request: &StreamingTileRequest) {
        self.inflight
            .remove(&StreamingRequestKey::from_request(request));
    }

    pub fn note_completed(&mut self) {
        self.stats.completed_total += 1;
    }

    pub fn note_failed(&mut self) {
        self.stats.failed_total += 1;
    }
}

fn streaming_allowed_for(
    attachment_label: &AttachmentLabel,
    settings: &TerrainStreamingSettings,
) -> bool {
    match attachment_label {
        AttachmentLabel::Height => settings.stream_height,
        AttachmentLabel::Custom(name) if name == "albedo" => settings.stream_imagery,
        AttachmentLabel::Custom(_) | AttachmentLabel::Empty(_) => false,
    }
}

pub fn collect_streaming_requests(
    mut atlases: Query<&mut TileAtlas>,
    settings: Res<TerrainStreamingSettings>,
    mut queue: ResMut<StreamingRequestQueue>,
) {
    for mut atlas in &mut atlases {
        for request in atlas.drain_streaming_requests() {
            queue.enqueue(request, &settings);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamingWorkerStats {
    pub started_total: u64,
    pub completed_total: u64,
    pub failed_total: u64,
    pub cache_writes_total: u64,
}

struct StreamingTaskResult {
    request: StreamingTileRequest,
    result: Result<PathBuf, StreamingTaskError>,
}

#[derive(Debug)]
enum StreamingTaskError {
    Provider(StreamingProviderError),
    CacheWrite(StreamingCacheWriteError),
    MissingCacheRoot,
    UnsupportedAttachment(AttachmentLabel),
}

#[derive(Resource, Default)]
pub struct StreamingWorker {
    inflight: Vec<Task<StreamingTaskResult>>,
    stats: StreamingWorkerStats,
}

impl StreamingWorker {
    pub fn stats(&self) -> &StreamingWorkerStats {
        &self.stats
    }
}

pub fn start_streaming_jobs(
    settings: Res<TerrainStreamingSettings>,
    terrain_settings: Res<TerrainSettings>,
    mut queue: ResMut<StreamingRequestQueue>,
    mut worker: ResMut<StreamingWorker>,
    gibs: Res<NasaGibsImageryProvider>,
    opentopography: Res<OpenTopographyHeightProvider>,
) {
    let available_slots = settings
        .max_inflight_requests
        .saturating_sub(worker.inflight.len());
    if available_slots == 0 {
        return;
    }

    let cache_root = terrain_settings.streaming_cache_root.clone();
    let gibs = gibs.clone();
    let opentopography = opentopography.clone();
    let stream_height = settings.stream_height;
    let asset_root = PathBuf::from("assets");
    let queued = queue.dequeue_batch(available_slots);

    for queued_request in queued {
        let request = queued_request.request;
        let cache_root = cache_root.clone();
        let gibs = gibs.clone();
        let opentopography = opentopography.clone();
        let stream_height = stream_height;
        let asset_root = asset_root.clone();
        worker.stats.started_total += 1;

        worker.inflight.push(IoTaskPool::get().spawn(async move {
            let result = match request.attachment_label {
                AttachmentLabel::Custom(ref name) if name == "albedo" => {
                    match cache_root.map(PathBuf::from) {
                        Some(cache_root) => materialize_imagery_request_into_cache(
                            &gibs,
                            &request,
                            &asset_root,
                            &cache_root,
                            stream_height,
                        ),
                        None => Err(StreamingTaskError::MissingCacheRoot),
                    }
                }
                AttachmentLabel::Height => match cache_root.map(PathBuf::from) {
                    Some(cache_root) if stream_height => materialize_request_into_cache(
                        &opentopography,
                        &request,
                        &asset_root,
                        &cache_root,
                    ),
                    Some(cache_root) => {
                        materialize_derived_height_into_cache(&request, &asset_root, &cache_root)
                    }
                    None => Err(StreamingTaskError::MissingCacheRoot),
                },
                _ => Err(StreamingTaskError::UnsupportedAttachment(
                    request.attachment_label.clone(),
                )),
            };

            StreamingTaskResult { request, result }
        }));
    }
}

pub fn finish_streaming_jobs(
    mut queue: ResMut<StreamingRequestQueue>,
    mut worker: ResMut<StreamingWorker>,
) {
    let mut remaining = Vec::with_capacity(worker.inflight.len());
    for mut task in std::mem::take(&mut worker.inflight) {
        if let Some(outcome) = bevy::tasks::block_on(poll_once(&mut task)) {
            queue.finish(&outcome.request);
            match outcome.result {
                Ok(path) => {
                    debug!(
                        "Streamed {:?} {:?} into cache at {}",
                        outcome.request.coordinate,
                        outcome.request.attachment_label,
                        path.display()
                    );
                    queue.note_completed();
                    worker.stats.completed_total += 1;
                    worker.stats.cache_writes_total += 1;
                }
                Err(error) => {
                    let message = describe_streaming_task_error(&error);
                    if should_downgrade_streaming_failure_log(&error, &outcome.request) {
                        debug!(
                            "Streaming request skipped for {:?} {:?}: {}",
                            outcome.request.coordinate,
                            outcome.request.attachment_label,
                            message
                        );
                    } else {
                        warn!(
                            "Streaming request failed for {:?} {:?}: {}",
                            outcome.request.coordinate,
                            outcome.request.attachment_label,
                            message
                        );
                    }
                    queue.note_failed();
                    worker.stats.failed_total += 1;
                }
            }
        } else {
            remaining.push(task);
        }
    }
    worker.inflight = remaining;
}

fn should_downgrade_streaming_failure_log(
    error: &StreamingTaskError,
    request: &StreamingTileRequest,
) -> bool {
    matches!(
        (error, &request.attachment_label),
        (
            StreamingTaskError::Provider(StreamingProviderError::Unsupported(reason)),
            AttachmentLabel::Custom(name)
        ) if name == "albedo"
            && reason.contains("tile longitude span crosses the antimeridian")
    )
}

fn materialize_request_into_cache<P: StreamingTileProvider>(
    provider: &P,
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
) -> Result<PathBuf, StreamingTaskError> {
    let tile = provider
        .materialize_tile(request)
        .map_err(StreamingTaskError::Provider)?;
    write_materialized_tile(asset_root, cache_root, &tile).map_err(StreamingTaskError::CacheWrite)
}

fn materialize_imagery_request_into_cache<P: StreamingTileProvider>(
    provider: &P,
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
    stream_height: bool,
) -> Result<PathBuf, StreamingTaskError> {
    materialize_missing_imagery_ancestor_chain_into_cache(
        provider,
        request,
        asset_root,
        cache_root,
        stream_height,
    )?;

    if !stream_height {
        ensure_derived_height_for_imagery_request(request, asset_root, cache_root)?;
    }

    materialize_request_into_cache(provider, request, asset_root, cache_root)
}

fn materialize_missing_imagery_ancestor_chain_into_cache<P: StreamingTileProvider>(
    provider: &P,
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
    stream_height: bool,
) -> Result<(), StreamingTaskError> {
    let mut ancestors = Vec::new();
    let mut coordinate = request.coordinate;
    while let Some(parent) = coordinate.parent() {
        ancestors.push(parent);
        coordinate = parent;
    }
    ancestors.reverse();

    for coordinate in ancestors {
        let ancestor_request = request_with_coordinate(request, coordinate);
        if !local_attachment_exists(asset_root, cache_root, &ancestor_request) {
            if !stream_height {
                ensure_derived_height_for_imagery_request(
                    &ancestor_request,
                    asset_root,
                    cache_root,
                )?;
            }
            materialize_request_into_cache(provider, &ancestor_request, asset_root, cache_root)?;
        }
    }

    Ok(())
}

fn ensure_derived_height_for_imagery_request(
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
) -> Result<(), StreamingTaskError> {
    if local_attachment_exists_with_label(
        asset_root,
        cache_root,
        &request.terrain_path,
        &AttachmentLabel::Height,
        request.coordinate,
    ) {
        return Ok(());
    }

    let derived_height_request = StreamingTileRequest {
        terrain_path: request.terrain_path.clone(),
        attachment_label: AttachmentLabel::Height,
        attachment_config: AttachmentConfig {
            texture_size: request.attachment_config.texture_size,
            border_size: request.attachment_config.border_size,
            mip_level_count: request.attachment_config.mip_level_count,
            mask: false,
            format: AttachmentFormat::R32F,
        },
        coordinate: request.coordinate,
        terrain_shape: request.terrain_shape,
        terrain_lod_count: request.terrain_lod_count,
        priority: request.priority,
    };

    materialize_derived_height_into_cache(&derived_height_request, asset_root, cache_root)?;
    Ok(())
}

fn request_with_coordinate(
    request: &StreamingTileRequest,
    coordinate: crate::math::TileCoordinate,
) -> StreamingTileRequest {
    StreamingTileRequest {
        terrain_path: request.terrain_path.clone(),
        attachment_label: request.attachment_label.clone(),
        attachment_config: AttachmentConfig {
            texture_size: request.attachment_config.texture_size,
            border_size: request.attachment_config.border_size,
            mip_level_count: request.attachment_config.mip_level_count,
            mask: request.attachment_config.mask,
            format: request.attachment_config.format,
        },
        coordinate,
        terrain_shape: request.terrain_shape,
        terrain_lod_count: request.terrain_lod_count,
        priority: request.priority,
    }
}

fn local_attachment_exists(
    asset_root: &Path,
    cache_root: &Path,
    request: &StreamingTileRequest,
) -> bool {
    local_attachment_exists_with_label(
        asset_root,
        cache_root,
        &request.terrain_path,
        &request.attachment_label,
        request.coordinate,
    )
}

fn local_attachment_exists_with_label(
    asset_root: &Path,
    cache_root: &Path,
    terrain_path: &str,
    attachment_label: &AttachmentLabel,
    coordinate: crate::math::TileCoordinate,
) -> bool {
    CacheFirstLocalTileSource::new(asset_root.to_path_buf(), Some(cache_root.to_path_buf()))
        .resolve_present_tile(&LocalTileRequest {
            terrain_path: terrain_path.to_string(),
            attachment_label: attachment_label.clone(),
            coordinate,
        })
        .is_some()
}

fn materialize_derived_height_into_cache(
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
) -> Result<PathBuf, StreamingTaskError> {
    let (source_samples, source_kind, source_coordinate, source_texture_size, source_border_size) =
        load_best_local_height_ancestor(request, asset_root, cache_root)?;

    let derived_heights = derive_child_height_tile(
        request,
        &source_samples,
        source_coordinate,
        source_texture_size,
        source_border_size,
    )
    .map_err(StreamingTaskError::Provider)?;
    let encoded_tile = encode_height_tiff(
        request.attachment_config.texture_size,
        request.attachment_config.texture_size,
        &derived_heights,
    )
    .map_err(StreamingTaskError::Provider)?;

    let tile = crate::streaming::MaterializedStreamingTile {
        bytes: encoded_tile,
        metadata: CachedTileMetadata {
            format_version: crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
            terrain_path: request.terrain_path.clone(),
            attachment_label: request.attachment_label.clone(),
            coordinate: request.coordinate,
            source: StreamingSourceDescriptor {
                source_id: match source_kind {
                    LocalTileSourceKind::StreamingCache => "local/derived_height_from_cache",
                    LocalTileSourceKind::StarterDataset => "local/derived_height_from_starter",
                }
                .to_string(),
                source_kind: match source_kind {
                    LocalTileSourceKind::StreamingCache => StreamingSourceKind::LocalCache,
                    LocalTileSourceKind::StarterDataset => StreamingSourceKind::LocalStarter,
                },
                attachment_kind: StreamedAttachmentKind::Height,
            },
            fetched_at_unix_ms: current_unix_ms(),
            expires_at_unix_ms: None,
            source_zoom: Some(source_coordinate.lod),
            source_revision: Some(source_coordinate.to_string()),
            source_content_hash: None,
            source_crs: None,
            encoding: CacheTileEncoding::Tiff,
        },
    };

    write_materialized_tile(asset_root, cache_root, &tile).map_err(StreamingTaskError::CacheWrite)
}

fn load_best_local_height_ancestor(
    request: &StreamingTileRequest,
    asset_root: &Path,
    cache_root: &Path,
) -> Result<
    (
        Vec<f32>,
        LocalTileSourceKind,
        crate::math::TileCoordinate,
        u32,
        u32,
    ),
    StreamingTaskError,
> {
    if request.attachment_label != AttachmentLabel::Height {
        return Err(StreamingTaskError::UnsupportedAttachment(
            request.attachment_label.clone(),
        ));
    }
    if request.attachment_config.format != AttachmentFormat::R32F {
        return Err(StreamingTaskError::Provider(
            StreamingProviderError::Unsupported(
                "derived local height currently requires R32F attachments".to_string(),
            ),
        ));
    }

    let resolver =
        CacheFirstLocalTileSource::new(asset_root.to_path_buf(), Some(cache_root.to_path_buf()));
    let mut source_coordinate = request.coordinate;
    loop {
        if let Some(resolved) = resolver.resolve_present_tile(&LocalTileRequest {
            terrain_path: request.terrain_path.clone(),
            attachment_label: AttachmentLabel::Height,
            coordinate: source_coordinate,
        }) {
            let bytes = std::fs::read(asset_root.join(&resolved.asset_path)).map_err(|error| {
                StreamingTaskError::Provider(StreamingProviderError::Transient(format!(
                    "failed to read local ancestor height tile {}: {error}",
                    resolved.asset_path.display()
                )))
            })?;
            let (width, height, samples) =
                decode_height_tiff(&bytes).map_err(StreamingTaskError::Provider)?;
            if width != height {
                return Err(StreamingTaskError::Provider(
                    StreamingProviderError::Permanent(format!(
                        "non-square local ancestor height tile {}x{}",
                        width, height
                    )),
                ));
            }
            return Ok((
                samples,
                resolved.source_kind,
                source_coordinate,
                width,
                request.attachment_config.border_size,
            ));
        }

        source_coordinate = source_coordinate.parent().ok_or_else(|| {
            StreamingTaskError::Provider(StreamingProviderError::Unavailable(
                "no local ancestor height tile was available for derived imagery-only refinement"
                    .to_string(),
            ))
        })?;
    }
}

fn derive_child_height_tile(
    request: &StreamingTileRequest,
    source_samples: &[f32],
    source_coordinate: crate::math::TileCoordinate,
    source_texture_size: u32,
    source_border_size: u32,
) -> Result<Vec<f32>, StreamingProviderError> {
    let target_size = request.attachment_config.texture_size;
    let target_border = request.attachment_config.border_size as f32;
    let target_center = request.attachment_config.center_size() as f32;
    let source_border = source_border_size as f32;
    let source_center = (source_texture_size - 2 * source_border_size) as f32;

    if source_texture_size == 0 || target_size == 0 {
        return Err(StreamingProviderError::Permanent(
            "derived local height encountered an empty texture".to_string(),
        ));
    }

    let lod_delta = request.coordinate.lod.saturating_sub(source_coordinate.lod);
    let descendant_scale = 1_u32 << lod_delta;
    let relative_xy = request.coordinate.xy - (source_coordinate.xy << lod_delta);
    let quadrant_origin_x = relative_xy.x as f32 / descendant_scale as f32;
    let quadrant_origin_y = relative_xy.y as f32 / descendant_scale as f32;
    let quadrant_span = 1.0 / descendant_scale as f32;

    let mut derived = Vec::with_capacity((target_size * target_size) as usize);
    for y in 0..target_size {
        let child_v = ((y as f32 - target_border) + 0.5) / target_center;
        let source_v = quadrant_origin_y + child_v * quadrant_span;
        let sample_y = source_border + source_v * source_center - 0.5;

        for x in 0..target_size {
            let child_u = ((x as f32 - target_border) + 0.5) / target_center;
            let source_u = quadrant_origin_x + child_u * quadrant_span;
            let sample_x = source_border + source_u * source_center - 0.5;
            derived.push(bilinear_sample_f32(
                source_samples,
                source_texture_size,
                source_texture_size,
                sample_x,
                sample_y,
            ));
        }
    }

    Ok(derived)
}

fn bilinear_sample_f32(
    samples: &[f32],
    width: u32,
    height: u32,
    sample_x: f32,
    sample_y: f32,
) -> f32 {
    let max_x = width.saturating_sub(1) as f32;
    let max_y = height.saturating_sub(1) as f32;
    let sample_x = sample_x.clamp(0.0, max_x);
    let sample_y = sample_y.clamp(0.0, max_y);

    let x0 = sample_x.floor() as usize;
    let y0 = sample_y.floor() as usize;
    let x1 = ((x0 + 1) as u32).min(width.saturating_sub(1)) as usize;
    let y1 = ((y0 + 1) as u32).min(height.saturating_sub(1)) as usize;
    let tx = sample_x.fract();
    let ty = sample_y.fract();

    let row_stride = width as usize;
    let top_left = samples[y0 * row_stride + x0];
    let top_right = samples[y0 * row_stride + x1];
    let bottom_left = samples[y1 * row_stride + x0];
    let bottom_right = samples[y1 * row_stride + x1];

    let top = top_left * (1.0 - tx) + top_right * tx;
    let bottom = bottom_left * (1.0 - tx) + bottom_right * tx;
    top * (1.0 - ty) + bottom * ty
}

fn decode_height_tiff(bytes: &[u8]) -> Result<(u32, u32, Vec<f32>), StreamingProviderError> {
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to construct local height TIFF decoder: {error}"
        ))
    })?;
    let (width, height) = decoder.dimensions().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to read local height TIFF dimensions: {error}"
        ))
    })?;

    let samples = match decoder.read_image().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to decode local height TIFF body: {error}"
        ))
    })? {
        DecodingResult::F32(values) => values,
        DecodingResult::U16(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U32(values) => values.into_iter().map(|value| value as f32).collect(),
        other => {
            return Err(StreamingProviderError::Permanent(format!(
                "unsupported local height TIFF sample type: {other:?}"
            )));
        }
    };

    Ok((width, height, samples))
}

fn encode_height_tiff(
    width: u32,
    height: u32,
    heights: &[f32],
) -> Result<Vec<u8>, StreamingProviderError> {
    let mut cursor = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut cursor).map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to create derived height TIFF encoder: {error}"
        ))
    })?;
    encoder
        .write_image::<colortype::Gray32Float>(width, height, heights)
        .map_err(|error| {
            StreamingProviderError::Permanent(format!(
                "failed to encode derived height TIFF: {error}"
            ))
        })?;
    Ok(cursor.into_inner())
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn describe_streaming_task_error(error: &StreamingTaskError) -> String {
    match error {
        StreamingTaskError::Provider(error) => error.to_string(),
        StreamingTaskError::CacheWrite(error) => error.to_string(),
        StreamingTaskError::MissingCacheRoot => {
            "streaming cache root is not configured".to_string()
        }
        StreamingTaskError::UnsupportedAttachment(label) => {
            format!(
                "no streaming provider is configured for attachment {:?}",
                label
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::TerrainShape,
        streaming::{
            CacheFirstLocalTileSource, CacheTileEncoding, LocalTileRequest, LocalTileSourceKind,
            MaterializedStreamingTile, StreamedAttachmentKind, StreamingRequestPriority,
            StreamingSourceAvailability, StreamingSourceDescriptor, StreamingSourceKind,
        },
        terrain_data::{AttachmentConfig, AttachmentFormat, AttachmentLabel},
    };
    use bevy::math::IVec2;
    use std::{
        fs,
        io::Cursor,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tiff::decoder::{Decoder, DecodingResult};
    use tiff::encoder::{TiffEncoder, colortype};

    fn albedo_request() -> StreamingTileRequest {
        StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig::default(),
            coordinate: crate::math::TileCoordinate::new(0, 2, IVec2::new(1, 1)),
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 4,
            priority: StreamingRequestPriority::Background,
        }
    }

    #[test]
    fn queue_dedupes_identical_requests() {
        let settings = TerrainStreamingSettings::online_imagery();
        let request = albedo_request();
        let mut queue = StreamingRequestQueue::default();

        assert!(queue.enqueue(request.clone(), &settings));
        assert!(!queue.enqueue(request.clone(), &settings));
        assert_eq!(queue.pending_count(), 1);
        assert_eq!(queue.stats().enqueued_total, 1);
        assert_eq!(queue.stats().deduped_total, 1);
    }

    #[test]
    fn queue_respects_offline_only_mode() {
        let settings = TerrainStreamingSettings::default();
        let mut queue = StreamingRequestQueue::default();

        assert!(!queue.enqueue(albedo_request(), &settings));
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.stats().dropped_offline_total, 1);
    }

    #[test]
    fn queue_filters_height_when_height_streaming_is_disabled() {
        let settings = TerrainStreamingSettings::online_imagery();
        let mut queue = StreamingRequestQueue::default();
        let mut request = albedo_request();
        request.attachment_label = AttachmentLabel::Height;

        assert!(!queue.enqueue(request, &settings));
        assert_eq!(queue.pending_count(), 0);
        assert_eq!(queue.stats().dropped_policy_total, 1);
    }

    #[test]
    fn queue_prefers_focused_requests_over_background_requests() {
        let settings = TerrainStreamingSettings::online_imagery();
        let mut queue = StreamingRequestQueue::default();

        let mut background = albedo_request();
        background.coordinate = crate::math::TileCoordinate::new(0, 3, IVec2::new(0, 0));
        background.priority = StreamingRequestPriority::Background;

        let mut focused = albedo_request();
        focused.coordinate = crate::math::TileCoordinate::new(0, 2, IVec2::new(1, 1));
        focused.priority = StreamingRequestPriority::Focused;

        assert!(queue.enqueue(background, &settings));
        assert!(queue.enqueue(focused.clone(), &settings));

        let drained = queue.dequeue_batch(1);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].request.priority, StreamingRequestPriority::Focused);
        assert_eq!(drained[0].request.coordinate, focused.coordinate);
    }

    #[test]
    fn queue_prefers_imagery_over_height_when_capacity_is_tight() {
        let settings =
            TerrainStreamingSettings::online_imagery_and_height().with_max_pending_requests(1);
        let mut queue = StreamingRequestQueue::default();

        let mut height_request = albedo_request();
        height_request.attachment_label = AttachmentLabel::Height;
        height_request.coordinate = crate::math::TileCoordinate::new(0, 6, IVec2::new(3, 5));

        let mut imagery_request = albedo_request();
        imagery_request.coordinate = crate::math::TileCoordinate::new(0, 4, IVec2::new(8, 2));

        assert!(queue.enqueue(height_request, &settings));
        assert!(queue.enqueue(imagery_request.clone(), &settings));

        let drained = queue.dequeue_batch(1);
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0].request.attachment_label,
            imagery_request.attachment_label
        );
        assert_eq!(drained[0].request.coordinate, imagery_request.coordinate);
    }

    #[test]
    fn queue_replaces_coarse_imagery_with_deeper_imagery() {
        let settings = TerrainStreamingSettings::online_imagery().with_max_pending_requests(1);
        let mut queue = StreamingRequestQueue::default();

        let mut coarse_request = albedo_request();
        coarse_request.coordinate = crate::math::TileCoordinate::new(0, 3, IVec2::new(2, 1));

        let mut deep_request = albedo_request();
        deep_request.coordinate = crate::math::TileCoordinate::new(0, 9, IVec2::new(40, 17));

        assert!(queue.enqueue(coarse_request, &settings));
        assert!(queue.enqueue(deep_request.clone(), &settings));

        let drained = queue.dequeue_batch(1);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].request.coordinate, deep_request.coordinate);
    }

    #[test]
    fn antimeridian_albedo_rejections_are_treated_as_expected_noise() {
        let error = StreamingTaskError::Provider(StreamingProviderError::Unsupported(
            "tile longitude span crosses the antimeridian; split-request planning is not implemented yet"
                .to_string(),
        ));

        assert!(should_downgrade_streaming_failure_log(
            &error,
            &albedo_request(),
        ));
    }

    #[test]
    fn non_albedo_or_non_antimeridian_failures_still_warn() {
        let antimeridian_error = StreamingTaskError::Provider(StreamingProviderError::Unsupported(
            "tile longitude span crosses the antimeridian; split-request planning is not implemented yet"
                .to_string(),
        ));
        let mut height_request = albedo_request();
        height_request.attachment_label = AttachmentLabel::Height;

        assert!(!should_downgrade_streaming_failure_log(
            &antimeridian_error,
            &height_request,
        ));

        let other_albedo_error = StreamingTaskError::Provider(StreamingProviderError::Unsupported(
            "provider is rate limited".to_string(),
        ));
        assert!(!should_downgrade_streaming_failure_log(
            &other_albedo_error,
            &albedo_request(),
        ));
    }

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("terrain_streaming_replay_{unique}"))
    }

    struct StubImageryProvider;

    fn encode_rgb_fixture_tiff(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let texel_count = (width * height) as usize;
        let mut bytes = Vec::with_capacity(texel_count * 3);
        for _ in 0..texel_count {
            bytes.extend_from_slice(&rgb);
        }

        let mut cursor = Cursor::new(Vec::new());
        let mut encoder = TiffEncoder::new(&mut cursor).unwrap();
        encoder
            .write_image::<colortype::RGB8>(width, height, &bytes)
            .unwrap();
        cursor.into_inner()
    }

    impl StreamingTileProvider for StubImageryProvider {
        fn descriptor(&self) -> StreamingSourceDescriptor {
            StreamingSourceDescriptor {
                source_id: "stub/imagery".to_string(),
                source_kind: StreamingSourceKind::Custom("stub".to_string()),
                attachment_kind: StreamedAttachmentKind::Imagery,
            }
        }

        fn availability(&self, _request: &StreamingTileRequest) -> StreamingSourceAvailability {
            StreamingSourceAvailability::Available
        }

        fn materialize_tile(
            &self,
            request: &StreamingTileRequest,
        ) -> Result<MaterializedStreamingTile, StreamingProviderError> {
            Ok(MaterializedStreamingTile {
                bytes: encode_rgb_fixture_tiff(
                    request.attachment_config.texture_size,
                    request.attachment_config.texture_size,
                    [12, 34, 56],
                ),
                metadata: crate::streaming::CachedTileMetadata {
                    format_version: crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
                    terrain_path: request.terrain_path.clone(),
                    attachment_label: request.attachment_label.clone(),
                    coordinate: request.coordinate,
                    source: self.descriptor(),
                    fetched_at_unix_ms: 1,
                    expires_at_unix_ms: None,
                    source_zoom: Some(request.coordinate.lod),
                    source_revision: None,
                    source_content_hash: None,
                    source_crs: Some("EPSG:4326".to_string()),
                    encoding: CacheTileEncoding::Tiff,
                },
            })
        }
    }

    #[test]
    fn warmed_cache_replays_without_network_requests() {
        let asset_root = unique_temp_dir();
        let cache_root = PathBuf::from("streaming_cache");
        let request = StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig {
                texture_size: 4,
                border_size: 0,
                mip_level_count: 1,
                mask: false,
                format: AttachmentFormat::Rgb8U,
            },
            coordinate: crate::math::TileCoordinate::new(0, 1, IVec2::new(0, 1)),
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 3,
            priority: StreamingRequestPriority::Background,
        };

        fs::create_dir_all(&asset_root).unwrap();
        let written = materialize_request_into_cache(
            &StubImageryProvider,
            &request,
            &asset_root,
            &cache_root,
        )
        .expect("cache warm should succeed");

        let resolver = CacheFirstLocalTileSource::new(asset_root.clone(), Some(cache_root));
        let resolved = resolver
            .resolve_present_tile(&LocalTileRequest {
                terrain_path: request.terrain_path.clone(),
                attachment_label: request.attachment_label.clone(),
                coordinate: request.coordinate,
            })
            .expect("offline replay should resolve the warmed cache");

        assert_eq!(resolved.asset_path, written);
        assert_eq!(resolved.source_kind, LocalTileSourceKind::StreamingCache);

        let mut decoder = Decoder::new(Cursor::new(fs::read(asset_root.join(written)).unwrap()))
            .expect("warmed tile should stay loader-compatible");
        match decoder.read_image().unwrap() {
            DecodingResult::U8(bytes) => assert_eq!(&bytes[0..3], &[12, 34, 56]),
            other => panic!("expected RGB8 TIFF bytes, got {other:?}"),
        }

        let mut queue = StreamingRequestQueue::default();
        assert!(!queue.enqueue(request, &TerrainStreamingSettings::default()));
        assert_eq!(queue.stats().dropped_offline_total, 1);

        fs::remove_dir_all(asset_root).unwrap();
    }
}
