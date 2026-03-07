use crate::terrain_data::{AttachmentData, AttachmentFormat, AttachmentTile, TileAtlas};
use bevy::{
    asset::{AssetServer, Assets, Handle},
    image::Image,
    prelude::*,
};
use slab::Slab;
use std::collections::HashMap;

struct LoadingTile {
    handle: Handle<Image>,
    tile: AttachmentTile,
    format: AttachmentFormat,
}

#[derive(Component)]
pub struct DefaultLoader {
    loading_tiles: Slab<LoadingTile>,
}

impl Default for DefaultLoader {
    fn default() -> Self {
        Self {
            loading_tiles: Slab::with_capacity(32),
        }
    }
}

impl DefaultLoader {
    fn inflight_counts(&self) -> HashMap<crate::math::TileCoordinate, usize> {
        let mut counts = HashMap::with_capacity(self.loading_tiles.len());
        for (_, tile) in self.loading_tiles.iter() {
            *counts.entry(tile.tile.coordinate).or_insert(0) += 1;
        }
        counts
    }

    fn to_load_next(
        &self,
        atlas: &TileAtlas,
        inflight_counts: &HashMap<crate::math::TileCoordinate, usize>,
    ) -> Option<usize> {
        let mut best_index = None;
        let mut best_priority = None;

        for (index, tile) in atlas.to_load.iter().enumerate() {
            let Some(priority) = atlas.loading_priority(tile.coordinate) else {
                continue;
            };

            let priority = (
                inflight_counts
                    .get(&tile.coordinate)
                    .copied()
                    .unwrap_or_default(),
                priority,
            );

            if best_priority.is_none_or(|best| priority > best) {
                best_priority = Some(priority);
                best_index = Some(index);
            }
        }

        best_index
    }

    fn cancel_stale(&mut self, atlas: &mut TileAtlas) {
        let mut canceled = 0_u64;
        self.loading_tiles.retain(|_, tile| {
            let keep = atlas.is_tile_requested(tile.tile.coordinate);
            if !keep {
                canceled += 1;
            }
            keep
        });
        atlas.note_canceled_inflight_attachment_loads(canceled);
        atlas.note_inflight_attachment_loads(self.loading_tiles.len());
    }

    fn finish_loading(
        &mut self,
        atlas: &mut TileAtlas,
        asset_server: &mut AssetServer,
        images: &mut Assets<Image>,
    ) {
        self.cancel_stale(atlas);
        self.loading_tiles.retain(|_, tile| {
            if asset_server.is_loaded(tile.handle.id()) {
                let image = images.get(tile.handle.id()).unwrap();
                let data = AttachmentData::from_bytes(image.data.as_ref().unwrap(), tile.format);
                atlas.tile_loaded(tile.tile.clone(), data);

                false
            } else if asset_server.load_state(tile.handle.id()).is_failed() {
                return false;
            } else {
                true
            }
        });
        atlas.note_inflight_attachment_loads(self.loading_tiles.len());
    }

    fn start_loading(&mut self, atlas: &mut TileAtlas, asset_server: &mut AssetServer) {
        self.cancel_stale(atlas);
        let mut inflight_counts = self.inflight_counts();
        while self.loading_tiles.len() < self.loading_tiles.capacity() {
            if let Some(index) = self.to_load_next(atlas, &inflight_counts) {
                let tile = atlas.to_load.swap_remove(index);
                let tile_coordinate = tile.coordinate;
                let attachment = &atlas.attachments[&tile.label];

                let path = tile
                    .coordinate
                    .path(&attachment.path.join(String::from(&tile.label)));

                self.loading_tiles.insert(LoadingTile {
                    handle: asset_server.load(path),
                    tile,
                    format: attachment.format,
                });
                *inflight_counts.entry(tile_coordinate).or_insert(0) += 1;
                atlas.note_inflight_attachment_loads(self.loading_tiles.len());
            } else {
                break;
            }
        }
    }
}

pub fn finish_loading(
    mut terrains: Query<(&mut TileAtlas, &mut DefaultLoader)>,
    mut asset_server: ResMut<AssetServer>,
    mut images: ResMut<Assets<Image>>,
) {
    for (mut tile_atlas, mut loader) in &mut terrains {
        loader.finish_loading(&mut tile_atlas, &mut asset_server, &mut images);
    }
}

pub fn start_loading(
    mut terrains: Query<(&mut TileAtlas, &mut DefaultLoader)>,
    mut asset_server: ResMut<AssetServer>,
) {
    for (mut tile_atlas, mut loader) in &mut terrains {
        loader.start_loading(&mut tile_atlas, &mut asset_server);
    }
}
