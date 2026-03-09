use crate::streaming::{
    CacheFreshnessPolicy, CachedTileMetadata, MaterializedStreamingTile, RegisteredStreamingSource,
    StreamingCacheManifest, StreamingCacheManifestError, cache_paths::cache_tile_asset_path,
};
use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum StreamingCacheWriteError {
    Io(std::io::Error),
    Manifest(StreamingCacheManifestError),
}

impl fmt::Display for StreamingCacheWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "streaming cache write I/O failed: {error}"),
            Self::Manifest(error) => write!(f, "streaming cache manifest failed: {error}"),
        }
    }
}

impl Error for StreamingCacheWriteError {}

impl From<std::io::Error> for StreamingCacheWriteError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<StreamingCacheManifestError> for StreamingCacheWriteError {
    fn from(value: StreamingCacheManifestError) -> Self {
        Self::Manifest(value)
    }
}

pub fn cache_terrain_root(cache_root: &Path, terrain_path: &str) -> PathBuf {
    cache_root.join(terrain_path)
}

pub fn write_materialized_tile(
    asset_root: &Path,
    cache_root: &Path,
    tile: &MaterializedStreamingTile,
) -> Result<PathBuf, StreamingCacheWriteError> {
    let tile_asset_path = cache_tile_asset_path(
        cache_root,
        &tile.metadata.terrain_path,
        &tile.metadata.attachment_label,
        tile.metadata.coordinate,
    );
    let tile_fs_path = asset_root.join(&tile_asset_path);
    if let Some(parent) = tile_fs_path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(&tile_fs_path, &tile.bytes)?;

    let sidecar_path = CachedTileMetadata::path_for_tile(&tile_fs_path);
    tile.metadata.save_file(&sidecar_path)?;

    ensure_registered_source(asset_root, cache_root, &tile.metadata)?;
    Ok(tile_asset_path)
}

fn ensure_registered_source(
    asset_root: &Path,
    cache_root: &Path,
    metadata: &CachedTileMetadata,
) -> Result<(), StreamingCacheWriteError> {
    let terrain_root = asset_root.join(cache_terrain_root(cache_root, &metadata.terrain_path));
    fs::create_dir_all(&terrain_root)?;
    let manifest_path = StreamingCacheManifest::path_for(&terrain_root);

    let mut manifest = if manifest_path.is_file() {
        StreamingCacheManifest::load_file(&manifest_path)?
    } else {
        StreamingCacheManifest {
            terrain_path: metadata.terrain_path.clone(),
            ..Default::default()
        }
    };

    if manifest.terrain_path.is_empty() {
        manifest.terrain_path = metadata.terrain_path.clone();
    }

    if manifest.terrain_path != metadata.terrain_path {
        return Err(StreamingCacheManifestError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "cache manifest terrain path '{}' does not match streamed tile terrain path '{}'",
                manifest.terrain_path, metadata.terrain_path
            ),
        ))
        .into());
    }

    if !manifest
        .sources
        .iter()
        .any(|source| source.descriptor == metadata.source)
    {
        manifest.sources.push(RegisteredStreamingSource {
            descriptor: metadata.source.clone(),
            freshness_policy: CacheFreshnessPolicy::default(),
        });
    }

    manifest.save_file(&manifest_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::TileCoordinate,
        streaming::{CacheTileEncoding, StreamingSourceDescriptor, StreamingSourceKind},
        terrain_data::AttachmentLabel,
    };
    use bevy::math::IVec2;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("terrain_streaming_cache_writer_{unique}"))
    }

    #[test]
    fn writer_creates_tile_sidecar_and_manifest() {
        let asset_root = unique_temp_dir();
        let cache_root = PathBuf::from("streaming_cache");
        let tile = MaterializedStreamingTile {
            bytes: b"tile".to_vec(),
            metadata: CachedTileMetadata {
                format_version: crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
                terrain_path: "terrains/earth".to_string(),
                attachment_label: AttachmentLabel::Custom("albedo".to_string()),
                coordinate: TileCoordinate::new(0, 1, IVec2::new(0, 1)),
                source: StreamingSourceDescriptor {
                    source_id: "nasa_gibs/modis_true_color".to_string(),
                    source_kind: StreamingSourceKind::NasaGibs,
                    attachment_kind: crate::streaming::StreamedAttachmentKind::Imagery,
                },
                fetched_at_unix_ms: 1,
                expires_at_unix_ms: None,
                source_zoom: None,
                source_revision: None,
                source_content_hash: None,
                source_crs: Some("EPSG:4326".to_string()),
                encoding: CacheTileEncoding::Tiff,
            },
        };

        let tile_asset_path = write_materialized_tile(&asset_root, &cache_root, &tile).unwrap();
        let tile_fs_path = asset_root.join(&tile_asset_path);

        assert!(tile_fs_path.is_file());
        assert!(CachedTileMetadata::path_for_tile(&tile_fs_path).is_file());
        assert!(
            StreamingCacheManifest::path_for(asset_root.join("streaming_cache/terrains/earth"))
                .is_file()
        );

        fs::remove_dir_all(asset_root).unwrap();
    }
}
