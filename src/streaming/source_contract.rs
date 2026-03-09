use crate::{
    math::{TerrainShape, TileCoordinate},
    streaming::cache_manifest::{
        CachedTileMetadata, StreamedAttachmentKind, StreamingSourceDescriptor,
    },
    terrain_data::{AttachmentConfig, AttachmentLabel},
};
use std::{error::Error, fmt};

#[derive(Clone, Debug)]
pub struct StreamingTileRequest {
    pub terrain_path: String,
    pub attachment_label: AttachmentLabel,
    pub attachment_config: AttachmentConfig,
    pub coordinate: TileCoordinate,
    pub terrain_shape: TerrainShape,
    pub terrain_lod_count: u32,
}

impl StreamingTileRequest {
    pub fn attachment_kind(&self) -> StreamedAttachmentKind {
        StreamedAttachmentKind::from(&self.attachment_label)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StreamingSourceAvailability {
    Available,
    Unavailable { reason: String },
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterializedStreamingTile {
    pub bytes: Vec<u8>,
    pub metadata: CachedTileMetadata,
}

#[derive(Debug)]
pub enum StreamingProviderError {
    Unavailable(String),
    Unsupported(String),
    Transient(String),
    Permanent(String),
}

impl fmt::Display for StreamingProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(message) => write!(f, "streaming provider is unavailable: {message}"),
            Self::Unsupported(message) => {
                write!(f, "streaming provider rejected the request: {message}")
            }
            Self::Transient(message) => {
                write!(
                    f,
                    "streaming provider encountered a transient failure: {message}"
                )
            }
            Self::Permanent(message) => {
                write!(
                    f,
                    "streaming provider encountered a permanent failure: {message}"
                )
            }
        }
    }
}

impl Error for StreamingProviderError {}

pub trait StreamingTileProvider: Send + Sync {
    fn descriptor(&self) -> StreamingSourceDescriptor;

    fn availability(&self, request: &StreamingTileRequest) -> StreamingSourceAvailability;

    fn materialize_tile(
        &self,
        request: &StreamingTileRequest,
    ) -> Result<MaterializedStreamingTile, StreamingProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::{CacheTileEncoding, StreamingSourceKind};
    use bevy::math::IVec2;

    struct StubProvider;

    impl StreamingTileProvider for StubProvider {
        fn descriptor(&self) -> StreamingSourceDescriptor {
            StreamingSourceDescriptor {
                source_id: "stub/source".to_string(),
                source_kind: StreamingSourceKind::Custom("stub".to_string()),
                attachment_kind: StreamedAttachmentKind::Imagery,
            }
        }

        fn availability(&self, request: &StreamingTileRequest) -> StreamingSourceAvailability {
            if request.attachment_kind() == StreamedAttachmentKind::Imagery {
                StreamingSourceAvailability::Available
            } else {
                StreamingSourceAvailability::Unavailable {
                    reason: "stub provider only serves imagery".to_string(),
                }
            }
        }

        fn materialize_tile(
            &self,
            request: &StreamingTileRequest,
        ) -> Result<MaterializedStreamingTile, StreamingProviderError> {
            match self.availability(request) {
                StreamingSourceAvailability::Available => Ok(MaterializedStreamingTile {
                    bytes: vec![1, 2, 3],
                    metadata: CachedTileMetadata {
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
                }),
                StreamingSourceAvailability::Unavailable { reason } => {
                    Err(StreamingProviderError::Unsupported(reason))
                }
                StreamingSourceAvailability::Unknown => Err(StreamingProviderError::Unavailable(
                    "availability unknown".to_string(),
                )),
            }
        }
    }

    #[test]
    fn request_attachment_kind_tracks_attachment_label() {
        let request = StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig::default(),
            coordinate: TileCoordinate::new(0, 1, IVec2::new(2, 3)),
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 6,
        };

        assert_eq!(request.attachment_kind(), StreamedAttachmentKind::Imagery);
    }

    #[test]
    fn provider_contract_can_describe_and_materialize_tiles() {
        let provider = StubProvider;
        let request = StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig::default(),
            coordinate: TileCoordinate::new(0, 1, IVec2::new(2, 3)),
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 6,
        };

        assert_eq!(
            provider.availability(&request),
            StreamingSourceAvailability::Available
        );
        let materialized = provider.materialize_tile(&request).unwrap();
        assert_eq!(materialized.bytes, vec![1, 2, 3]);
        assert_eq!(materialized.metadata.source.source_id, "stub/source");
    }
}
