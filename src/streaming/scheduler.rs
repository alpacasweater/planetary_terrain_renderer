use crate::{
    plugin::TerrainSettings,
    streaming::{
        NasaGibsImageryProvider, StreamingProviderError, StreamingTileProvider,
        cache_writer::{StreamingCacheWriteError, write_materialized_tile},
        source_contract::StreamingTileRequest,
    },
    terrain_data::{AttachmentLabel, TileAtlas},
};
use bevy::{
    platform::collections::{HashMap, HashSet},
    prelude::*,
    tasks::{IoTaskPool, Task, poll_once},
};
use std::cmp::Reverse;
use std::path::PathBuf;

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

        if self.pending.len() >= settings.max_pending_requests {
            self.stats.dropped_capacity_total += 1;
            return false;
        }

        let key = StreamingRequestKey::from_request(&request);
        if self.pending.contains_key(&key) || self.inflight.contains(&key) {
            self.stats.deduped_total += 1;
            return false;
        }

        let sequence = self.next_sequence;
        self.next_sequence += 1;
        self.pending
            .insert(key, QueuedStreamingRequest { request, sequence });
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
            (Reverse(request.request.coordinate.lod), request.sequence)
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
) {
    let available_slots = settings
        .max_inflight_requests
        .saturating_sub(worker.inflight.len());
    if available_slots == 0 {
        return;
    }

    let cache_root = terrain_settings.streaming_cache_root.clone();
    let gibs = gibs.clone();
    let asset_root = PathBuf::from("assets");
    let queued = queue.dequeue_batch(available_slots);

    for queued_request in queued {
        let request = queued_request.request;
        let cache_root = cache_root.clone();
        let gibs = gibs.clone();
        let asset_root = asset_root.clone();
        worker.stats.started_total += 1;

        worker.inflight.push(IoTaskPool::get().spawn(async move {
            let result = match request.attachment_label {
                AttachmentLabel::Custom(ref name) if name == "albedo" => {
                    match cache_root.map(PathBuf::from) {
                        Some(cache_root) => match gibs.materialize_tile(&request) {
                            Ok(tile) => write_materialized_tile(&asset_root, &cache_root, &tile)
                                .map_err(StreamingTaskError::CacheWrite),
                            Err(error) => Err(StreamingTaskError::Provider(error)),
                        },
                        None => Err(StreamingTaskError::MissingCacheRoot),
                    }
                }
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
                    warn!(
                        "Streaming request failed for {:?} {:?}: {}",
                        outcome.request.coordinate,
                        outcome.request.attachment_label,
                        describe_streaming_task_error(&error)
                    );
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
        terrain_data::{AttachmentConfig, AttachmentLabel},
    };
    use bevy::math::IVec2;

    fn albedo_request() -> StreamingTileRequest {
        StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig::default(),
            coordinate: crate::math::TileCoordinate::new(0, 2, IVec2::new(1, 1)),
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 4,
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
        assert_eq!(queue.stats().dropped_policy_total, 1);
    }
}
