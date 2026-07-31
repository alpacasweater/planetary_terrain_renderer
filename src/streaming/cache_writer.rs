use crate::streaming::{
    CacheFreshnessPolicy, CachedTileMetadata, MaterializedStreamingTile, RegisteredStreamingSource,
    StreamingCacheManifest, StreamingCacheManifestError, cache_manifest::atomic_write_bytes,
    cache_paths::cache_tile_asset_path,
};
use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Serializes the manifest read-modify-write across concurrent streaming tasks. Manifest
/// updates are rare (once per newly-seen source per terrain), so a single process-wide lock is
/// cheap and prevents the lost-update / torn-file races that produced malformed manifests.
static MANIFEST_WRITE_LOCK: Mutex<()> = Mutex::new(());

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
    crate::streaming::cache_paths::versioned_cache_root(cache_root).join(terrain_path)
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

    // Write the tile and its sidecar atomically (temp + rename) so the main thread, which polls
    // for the final path with is_file, never loads a half-written tile.
    atomic_write_bytes(&tile_fs_path, &tile.bytes)?;

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
    // Serialize the whole read-modify-write so concurrent tasks cannot lose each other's source
    // registrations or observe a torn manifest. A poisoned lock still lets us proceed -- the
    // manifest write itself is atomic, so a panic mid-update cannot have corrupted the file.
    let _guard = MANIFEST_WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let terrain_root = asset_root.join(cache_terrain_root(cache_root, &metadata.terrain_path));
    fs::create_dir_all(&terrain_root)?;
    let manifest_path = StreamingCacheManifest::path_for(&terrain_root);

    let mut manifest = if manifest_path.is_file() {
        match StreamingCacheManifest::load_file(&manifest_path) {
            Ok(manifest) => manifest,
            Err(StreamingCacheManifestError::Ron(error)) => {
                bevy::log::warn!(
                    "Streaming cache manifest at {} was malformed ({}). Recreating it so streamed tiles can keep writing.",
                    manifest_path.display(),
                    error
                );
                StreamingCacheManifest {
                    terrain_path: metadata.terrain_path.clone(),
                    ..Default::default()
                }
            }
            Err(error) => return Err(error.into()),
        }
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
            StreamingCacheManifest::path_for(asset_root.join(cache_terrain_root(
                Path::new("streaming_cache"),
                "terrains/earth"
            )))
            .is_file()
        );

        fs::remove_dir_all(asset_root).unwrap();
    }

    #[test]
    fn concurrent_writes_keep_manifest_valid_and_register_all_sources() {
        use std::sync::Arc;
        use std::thread;

        let asset_root = Arc::new(unique_temp_dir());
        let cache_root = Arc::new(PathBuf::from("streaming_cache"));

        let mut handles = Vec::new();
        for thread_index in 0..8_u32 {
            let asset_root = Arc::clone(&asset_root);
            let cache_root = Arc::clone(&cache_root);
            handles.push(thread::spawn(move || {
                let is_height = thread_index % 2 == 0;
                let (label, source) = if is_height {
                    (
                        AttachmentLabel::Height,
                        StreamingSourceDescriptor {
                            source_id: "opentopography/aw3d30_e".to_string(),
                            source_kind: StreamingSourceKind::OpenTopography,
                            attachment_kind: crate::streaming::StreamedAttachmentKind::Height,
                        },
                    )
                } else {
                    (
                        AttachmentLabel::Custom("albedo".to_string()),
                        StreamingSourceDescriptor {
                            source_id: "nasa_gibs/modis_true_color".to_string(),
                            source_kind: StreamingSourceKind::NasaGibs,
                            attachment_kind: crate::streaming::StreamedAttachmentKind::Imagery,
                        },
                    )
                };

                for tile_index in 0..16_u32 {
                    let tile = MaterializedStreamingTile {
                        bytes: vec![thread_index as u8; 64],
                        metadata: CachedTileMetadata {
                            format_version:
                                crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
                            terrain_path: "terrains/earth".to_string(),
                            attachment_label: label.clone(),
                            coordinate: TileCoordinate::new(
                                thread_index % 6,
                                3,
                                IVec2::new(tile_index as i32, thread_index as i32),
                            ),
                            source: source.clone(),
                            fetched_at_unix_ms: 1,
                            expires_at_unix_ms: None,
                            source_zoom: None,
                            source_revision: None,
                            source_content_hash: None,
                            source_crs: None,
                            encoding: CacheTileEncoding::Tiff,
                        },
                    };
                    write_materialized_tile(&asset_root, &cache_root, &tile)
                        .expect("concurrent cache writes should succeed");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("writer thread should not panic");
        }

        let manifest =
            StreamingCacheManifest::load_file(StreamingCacheManifest::path_for(asset_root.join(
                cache_terrain_root(Path::new("streaming_cache"), "terrains/earth"),
            )))
            .expect("manifest must remain valid RON under concurrent writes");
        assert_eq!(
            manifest.sources.len(),
            2,
            "both distinct sources should survive the concurrent read-modify-write"
        );

        fs::remove_dir_all(&*asset_root).unwrap();
    }

    #[test]
    fn writer_recovers_from_malformed_manifest() {
        let asset_root = unique_temp_dir();
        let cache_root = PathBuf::from("streaming_cache");
        let terrain_root = asset_root.join(cache_terrain_root(&cache_root, "terrains/earth"));
        fs::create_dir_all(&terrain_root).unwrap();
        fs::write(
            StreamingCacheManifest::path_for(&terrain_root),
            "{ definitely_not_ron: true }\n",
        )
        .unwrap();

        let tile = MaterializedStreamingTile {
            bytes: b"tile".to_vec(),
            metadata: CachedTileMetadata {
                format_version: crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
                terrain_path: "terrains/earth".to_string(),
                attachment_label: AttachmentLabel::Height,
                coordinate: TileCoordinate::new(1, 2, IVec2::new(3, 4)),
                source: StreamingSourceDescriptor {
                    source_id: "opentopography/aw3d30_e".to_string(),
                    source_kind: StreamingSourceKind::OpenTopography,
                    attachment_kind: crate::streaming::StreamedAttachmentKind::Height,
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

        write_materialized_tile(&asset_root, &cache_root, &tile)
            .expect("writer should recreate malformed manifests");

        let manifest =
            StreamingCacheManifest::load_file(StreamingCacheManifest::path_for(asset_root.join(
                cache_terrain_root(Path::new("streaming_cache"), "terrains/earth"),
            )))
            .expect("recreated manifest should be valid RON");
        assert_eq!(manifest.terrain_path, "terrains/earth");
        assert_eq!(manifest.sources.len(), 1);

        fs::remove_dir_all(asset_root).unwrap();
    }
}
