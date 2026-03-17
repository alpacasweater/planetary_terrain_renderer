use crate::{math::TileCoordinate, terrain_data::AttachmentLabel};
use ron::{de::from_str, ser::to_string_pretty};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

pub const CURRENT_STREAMING_CACHE_FORMAT_VERSION: u32 = 1;
pub const STREAMING_CACHE_MANIFEST_FILE_NAME: &str = "streaming_cache_manifest.ron";
pub const STREAMING_TILE_METADATA_EXTENSION: &str = "tile-cache.ron";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum StreamingSourceKind {
    NasaGibs,
    Sentinel2Cog,
    OpenTopography,
    LocalStarter,
    LocalCache,
    Custom(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum StreamedAttachmentKind {
    Height,
    Imagery,
    Custom(String),
}

impl From<&AttachmentLabel> for StreamedAttachmentKind {
    fn from(value: &AttachmentLabel) -> Self {
        match value {
            AttachmentLabel::Height => Self::Height,
            AttachmentLabel::Custom(name) if name == "albedo" => Self::Imagery,
            AttachmentLabel::Custom(name) => Self::Custom(name.clone()),
            AttachmentLabel::Empty(index) => Self::Custom(format!("empty_{index}")),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StreamingSourceDescriptor {
    pub source_id: String,
    pub source_kind: StreamingSourceKind,
    pub attachment_kind: StreamedAttachmentKind,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct CacheFreshnessPolicy {
    pub max_age_seconds: Option<u64>,
    pub revalidate_on_startup: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RegisteredStreamingSource {
    pub descriptor: StreamingSourceDescriptor,
    pub freshness_policy: CacheFreshnessPolicy,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StreamingCacheManifest {
    pub format_version: u32,
    pub terrain_path: String,
    pub sources: Vec<RegisteredStreamingSource>,
}

impl Default for StreamingCacheManifest {
    fn default() -> Self {
        Self {
            format_version: CURRENT_STREAMING_CACHE_FORMAT_VERSION,
            terrain_path: String::new(),
            sources: Vec::new(),
        }
    }
}

impl StreamingCacheManifest {
    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self, StreamingCacheManifestError> {
        let encoded = fs::read_to_string(path)?;
        parse_ron_document(&encoded)
    }

    pub fn save_file<P: AsRef<Path>>(&self, path: P) -> Result<(), StreamingCacheManifestError> {
        let encoded = to_string_pretty(self, Default::default())?;
        fs::write(path, encoded)?;
        Ok(())
    }

    pub fn path_for<P: AsRef<Path>>(cache_terrain_root: P) -> PathBuf {
        cache_terrain_root
            .as_ref()
            .join(STREAMING_CACHE_MANIFEST_FILE_NAME)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum CacheTileEncoding {
    Tiff,
    Custom(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CachedTileMetadata {
    pub format_version: u32,
    pub terrain_path: String,
    pub attachment_label: AttachmentLabel,
    pub coordinate: TileCoordinate,
    pub source: StreamingSourceDescriptor,
    pub fetched_at_unix_ms: u64,
    pub expires_at_unix_ms: Option<u64>,
    pub source_zoom: Option<u32>,
    pub source_revision: Option<String>,
    pub source_content_hash: Option<String>,
    pub source_crs: Option<String>,
    pub encoding: CacheTileEncoding,
}

impl CachedTileMetadata {
    pub fn path_for_tile<P: AsRef<Path>>(tile_path: P) -> PathBuf {
        tile_path
            .as_ref()
            .with_extension(STREAMING_TILE_METADATA_EXTENSION)
    }

    pub fn load_file<P: AsRef<Path>>(path: P) -> Result<Self, StreamingCacheManifestError> {
        let encoded = fs::read_to_string(path)?;
        parse_ron_document(&encoded)
    }

    pub fn save_file<P: AsRef<Path>>(&self, path: P) -> Result<(), StreamingCacheManifestError> {
        let encoded = to_string_pretty(self, Default::default())?;
        fs::write(path, encoded)?;
        Ok(())
    }

    pub fn is_usable_with(
        &self,
        source: &StreamingSourceDescriptor,
        freshness_policy: &CacheFreshnessPolicy,
        now_unix_ms: u64,
    ) -> bool {
        if self.format_version != CURRENT_STREAMING_CACHE_FORMAT_VERSION {
            return false;
        }

        if &self.source != source {
            return false;
        }

        if let Some(max_age_seconds) = freshness_policy.max_age_seconds {
            let max_age_ms = max_age_seconds.saturating_mul(1000);
            if now_unix_ms.saturating_sub(self.fetched_at_unix_ms) > max_age_ms {
                return false;
            }
        }

        if let Some(expires_at_unix_ms) = self.expires_at_unix_ms {
            if now_unix_ms > expires_at_unix_ms {
                return false;
            }
        }

        true
    }
}

#[derive(Debug)]
pub enum StreamingCacheManifestError {
    Io(std::io::Error),
    Ron(ron::error::SpannedError),
    RonSerialize(ron::Error),
}

impl fmt::Display for StreamingCacheManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "streaming cache I/O failed: {error}"),
            Self::Ron(error) => write!(f, "streaming cache ron failed: {error}"),
            Self::RonSerialize(error) => {
                write!(f, "streaming cache ron serialization failed: {error}")
            }
        }
    }
}

impl Error for StreamingCacheManifestError {}

impl From<std::io::Error> for StreamingCacheManifestError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ron::error::SpannedError> for StreamingCacheManifestError {
    fn from(value: ron::error::SpannedError) -> Self {
        Self::Ron(value)
    }
}

impl From<ron::Error> for StreamingCacheManifestError {
    fn from(value: ron::Error) -> Self {
        Self::RonSerialize(value)
    }
}

fn parse_ron_document<T: DeserializeOwned>(encoded: &str) -> Result<T, StreamingCacheManifestError> {
    Ok(from_str(strip_utf8_bom(encoded))?)
}

fn strip_utf8_bom(encoded: &str) -> &str {
    encoded.strip_prefix('\u{feff}').unwrap_or(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::TileCoordinate;
    use bevy::math::IVec2;

    #[test]
    fn cache_manifest_default_uses_current_version() {
        let manifest = StreamingCacheManifest::default();
        assert_eq!(
            manifest.format_version,
            CURRENT_STREAMING_CACHE_FORMAT_VERSION
        );
    }

    #[test]
    fn tile_metadata_path_uses_sidecar_extension() {
        let path = CachedTileMetadata::path_for_tile("/tmp/0/0_0/0_0_0_0.tif");
        assert_eq!(path, PathBuf::from("/tmp/0/0_0/0_0_0_0.tile-cache.ron"));
    }

    #[test]
    fn metadata_roundtrip_preserves_source_identity() {
        let metadata = CachedTileMetadata {
            format_version: CURRENT_STREAMING_CACHE_FORMAT_VERSION,
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            coordinate: TileCoordinate::new(2, 3, IVec2::new(4, 5)),
            source: StreamingSourceDescriptor {
                source_id: "nasa_gibs/modis_true_color".to_string(),
                source_kind: StreamingSourceKind::NasaGibs,
                attachment_kind: StreamedAttachmentKind::Imagery,
            },
            fetched_at_unix_ms: 10,
            expires_at_unix_ms: Some(20),
            source_zoom: Some(6),
            source_revision: Some("rev-a".to_string()),
            source_content_hash: Some("hash".to_string()),
            source_crs: Some("EPSG:4326".to_string()),
            encoding: CacheTileEncoding::Tiff,
        };

        let encoded = to_string_pretty(&metadata, Default::default()).unwrap();
        let decoded: CachedTileMetadata = from_str(&encoded).unwrap();
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn manifest_load_tolerates_utf8_bom() {
        let encoded = "\u{feff}(\n    format_version: 1,\n    terrain_path: \"terrains/earth\",\n    sources: [],\n)\n";

        let decoded: StreamingCacheManifest = parse_ron_document(encoded).unwrap();
        assert_eq!(decoded.format_version, 1);
        assert_eq!(decoded.terrain_path, "terrains/earth");
        assert!(decoded.sources.is_empty());
    }

    #[test]
    fn metadata_load_tolerates_utf8_bom() {
        let metadata = CachedTileMetadata {
            format_version: CURRENT_STREAMING_CACHE_FORMAT_VERSION,
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Height,
            coordinate: TileCoordinate::new(2, 3, IVec2::new(4, 5)),
            source: StreamingSourceDescriptor {
                source_id: "opentopography/aw3d30_e".to_string(),
                source_kind: StreamingSourceKind::OpenTopography,
                attachment_kind: StreamedAttachmentKind::Height,
            },
            fetched_at_unix_ms: 10,
            expires_at_unix_ms: None,
            source_zoom: None,
            source_revision: None,
            source_content_hash: None,
            source_crs: Some("EPSG:4326".to_string()),
            encoding: CacheTileEncoding::Tiff,
        };
        let encoded = format!(
            "\u{feff}{}",
            to_string_pretty(&metadata, Default::default()).unwrap()
        );

        let decoded: CachedTileMetadata = parse_ron_document(&encoded).unwrap();
        assert_eq!(decoded, metadata);
    }

    #[test]
    fn metadata_usability_rejects_version_source_and_expiry_mismatches() {
        let source = StreamingSourceDescriptor {
            source_id: "nasa_gibs/modis_true_color".to_string(),
            source_kind: StreamingSourceKind::NasaGibs,
            attachment_kind: StreamedAttachmentKind::Imagery,
        };
        let freshness_policy = CacheFreshnessPolicy {
            max_age_seconds: Some(1),
            revalidate_on_startup: false,
        };

        let valid = CachedTileMetadata {
            format_version: CURRENT_STREAMING_CACHE_FORMAT_VERSION,
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            coordinate: TileCoordinate::new(0, 0, IVec2::ZERO),
            source: source.clone(),
            fetched_at_unix_ms: 1_000,
            expires_at_unix_ms: Some(2_500),
            source_zoom: None,
            source_revision: None,
            source_content_hash: None,
            source_crs: None,
            encoding: CacheTileEncoding::Tiff,
        };
        assert!(valid.is_usable_with(&source, &freshness_policy, 1_500));

        let mut stale = valid.clone();
        stale.fetched_at_unix_ms = 0;
        assert!(!stale.is_usable_with(&source, &freshness_policy, 2_000));

        let mut wrong_source = valid.clone();
        wrong_source.source.source_id = "sentinel-2/default".to_string();
        assert!(!wrong_source.is_usable_with(&source, &freshness_policy, 1_500));

        let mut expired = valid;
        expired.expires_at_unix_ms = Some(1_100);
        assert!(!expired.is_usable_with(&source, &freshness_policy, 1_500));
    }
}
