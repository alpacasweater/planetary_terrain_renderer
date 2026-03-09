use crate::{
    streaming::cache_paths::{cache_tile_asset_path, starter_tile_asset_path},
    terrain_data::AttachmentLabel,
};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalTileRequest {
    pub terrain_path: String,
    pub attachment_label: AttachmentLabel,
    pub coordinate: crate::math::TileCoordinate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalTileSourceKind {
    StreamingCache,
    StarterDataset,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedLocalTile {
    pub asset_path: PathBuf,
    pub source_kind: LocalTileSourceKind,
}

pub trait LocalTileSource {
    fn resolve_tile(&self, request: &LocalTileRequest) -> Option<ResolvedLocalTile>;
}

pub struct CacheFirstLocalTileSource {
    asset_root: PathBuf,
    cache_root: Option<PathBuf>,
}

impl CacheFirstLocalTileSource {
    pub fn new(asset_root: PathBuf, cache_root: Option<PathBuf>) -> Self {
        Self {
            asset_root,
            cache_root,
        }
    }

    fn resolve_cache_tile(&self, request: &LocalTileRequest) -> Option<ResolvedLocalTile> {
        let cache_root = self.cache_root.as_ref()?;
        let asset_path = cache_tile_asset_path(
            cache_root,
            &request.terrain_path,
            &request.attachment_label,
            request.coordinate,
        );
        self.file_if_exists(asset_path, LocalTileSourceKind::StreamingCache)
    }

    fn resolve_starter_tile(&self, request: &LocalTileRequest) -> Option<ResolvedLocalTile> {
        let asset_path = starter_tile_asset_path(
            &request.terrain_path,
            &request.attachment_label,
            request.coordinate,
        );
        Some(ResolvedLocalTile {
            asset_path,
            source_kind: LocalTileSourceKind::StarterDataset,
        })
    }

    fn file_if_exists(
        &self,
        asset_path: PathBuf,
        source_kind: LocalTileSourceKind,
    ) -> Option<ResolvedLocalTile> {
        let filesystem_path = self.asset_root.join(&asset_path);
        filesystem_path.is_file().then_some(ResolvedLocalTile {
            asset_path,
            source_kind,
        })
    }
}

impl LocalTileSource for CacheFirstLocalTileSource {
    fn resolve_tile(&self, request: &LocalTileRequest) -> Option<ResolvedLocalTile> {
        self.resolve_cache_tile(request)
            .or_else(|| self.resolve_starter_tile(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::IVec2;
    use std::{
        fs,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("terrain_tile_source_{unique}"))
    }

    #[test]
    fn resolver_prefers_cache_over_starter_when_both_exist() {
        let asset_root = unique_temp_dir();
        let request = LocalTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            coordinate: crate::math::TileCoordinate::new(0, 1, IVec2::new(2, 3)),
        };
        let cache_tile = cache_tile_asset_path(
            Path::new("streaming_cache"),
            &request.terrain_path,
            &request.attachment_label,
            request.coordinate,
        );
        let starter_tile = starter_tile_asset_path(
            &request.terrain_path,
            &request.attachment_label,
            request.coordinate,
        );
        fs::create_dir_all(asset_root.join(cache_tile.parent().unwrap())).unwrap();
        fs::create_dir_all(asset_root.join(starter_tile.parent().unwrap())).unwrap();
        fs::write(asset_root.join(&cache_tile), b"cache").unwrap();
        fs::write(asset_root.join(&starter_tile), b"starter").unwrap();

        let resolver = CacheFirstLocalTileSource::new(
            asset_root.clone(),
            Some(PathBuf::from("streaming_cache")),
        );
        let resolved = resolver.resolve_tile(&request).unwrap();
        assert_eq!(resolved.asset_path, cache_tile);
        assert_eq!(resolved.source_kind, LocalTileSourceKind::StreamingCache);

        fs::remove_dir_all(asset_root).unwrap();
    }

    #[test]
    fn resolver_falls_back_to_starter_when_cache_is_missing() {
        let asset_root = unique_temp_dir();
        let request = LocalTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Height,
            coordinate: crate::math::TileCoordinate::new(0, 1, IVec2::new(2, 3)),
        };
        let starter_tile = starter_tile_asset_path(
            &request.terrain_path,
            &request.attachment_label,
            request.coordinate,
        );
        fs::create_dir_all(asset_root.join(starter_tile.parent().unwrap())).unwrap();
        fs::write(asset_root.join(&starter_tile), b"starter").unwrap();

        let resolver = CacheFirstLocalTileSource::new(
            asset_root.clone(),
            Some(PathBuf::from("streaming_cache")),
        );
        let resolved = resolver.resolve_tile(&request).unwrap();
        assert_eq!(resolved.asset_path, starter_tile);
        assert_eq!(resolved.source_kind, LocalTileSourceKind::StarterDataset);

        fs::remove_dir_all(asset_root).unwrap();
    }
}
