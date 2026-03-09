use crate::{
    streaming::source_contract::StreamingTileRequest,
    terrain_data::{AttachmentLabel, TileAtlas},
};
use bevy::{
    platform::collections::{HashMap, HashSet},
    prelude::*,
};
use std::cmp::Reverse;

#[derive(Clone, Debug, PartialEq, Eq, Resource)]
pub struct TerrainStreamingSettings {
    pub offline_only: bool,
    pub stream_imagery: bool,
    pub stream_height: bool,
    pub max_pending_requests: usize,
}

impl Default for TerrainStreamingSettings {
    fn default() -> Self {
        Self {
            offline_only: true,
            stream_imagery: true,
            stream_height: false,
            max_pending_requests: 1024,
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
        }
    }

    pub fn online_imagery_and_height() -> Self {
        Self {
            offline_only: false,
            stream_imagery: true,
            stream_height: true,
            max_pending_requests: 1024,
        }
    }

    pub fn with_max_pending_requests(mut self, max_pending_requests: usize) -> Self {
        self.max_pending_requests = max_pending_requests;
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
