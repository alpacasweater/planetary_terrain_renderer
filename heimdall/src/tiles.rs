//! WebMercator (slippy-map) quadtree math — the tiling **index**, never a coordinate frame.
//!
//! All free terrain/imagery sources (Mapterhorn, EOX Sentinel-2, NASA GIBS XYZ, OSM-style
//! servers) publish tiles in this scheme, so heimdall tiles in it too and uses fetched tiles
//! as-is. Positions, however, live on the WGS84 ellipsoid via [`crate::frame`]; this module only
//! answers "which tile covers this lon/lat" and "what geographic extent does this tile span".
//!
//! Conventions: XYZ / "slippy" scheme — `z` zoom (0 = one world tile), `x` grows east from the
//! antimeridian, `y` grows **south** from ~85.0511°N. Google/OSM y-order (not TMS).

use std::f64::consts::PI;

/// Latitude bound of the WebMercator square world, `atan(sinh(π))` in degrees.
pub const MERCATOR_MAX_LAT_DEG: f64 = 85.051_128_779_806_59;

/// Identifies one tile of the WebMercator quadtree (slippy `z/x/y`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl TileId {
    /// Number of tiles along one axis at this zoom (`2^z`).
    pub fn tiles_per_axis(z: u8) -> u32 {
        1u32 << z
    }

    /// The parent tile one zoom coarser, or `None` at z0.
    pub fn parent(self) -> Option<TileId> {
        (self.z > 0).then(|| TileId {
            z: self.z - 1,
            x: self.x / 2,
            y: self.y / 2,
        })
    }

    /// The four children one zoom finer.
    pub fn children(self) -> [TileId; 4] {
        let (z, x, y) = (self.z + 1, self.x * 2, self.y * 2);
        [
            TileId { z, x, y },
            TileId { z, x: x + 1, y },
            TileId { z, x, y: y + 1 },
            TileId {
                z,
                x: x + 1,
                y: y + 1,
            },
        ]
    }
}

/// The tile containing `(lon_deg, lat_deg)` at zoom `z`. Latitude is clamped into the Mercator
/// world; longitude is wrapped into [−180, 180).
pub fn tile_for_lon_lat(lon_deg: f64, lat_deg: f64, z: u8) -> TileId {
    let n = f64::from(TileId::tiles_per_axis(z));
    let lon = wrap_lon(lon_deg);
    let lat = lat_deg.clamp(-MERCATOR_MAX_LAT_DEG, MERCATOR_MAX_LAT_DEG);

    let x = ((lon + 180.0) / 360.0 * n).floor();
    let y = ((1.0 - lat.to_radians().tan().asinh() / PI) / 2.0 * n).floor();

    let max = TileId::tiles_per_axis(z) - 1;
    TileId {
        z,
        x: (x as i64).clamp(0, i64::from(max)) as u32,
        y: (y as i64).clamp(0, i64::from(max)) as u32,
    }
}

/// Geographic extent of a tile as `[west, south, east, north]` degrees. The north/west edge is
/// exact; south/east are the next tile's north/west edge.
pub fn tile_bounds_lon_lat(tile: TileId) -> [f64; 4] {
    let (west, north) = tile_nw_corner(tile);
    let (east, south) = tile_nw_corner(TileId {
        z: tile.z,
        x: tile.x + 1,
        y: tile.y + 1,
    });
    [west, south, east, north]
}

/// Longitude/latitude of a tile's north-west corner. Accepts `x`/`y` equal to `2^z` so callers
/// can compute the closing edge of the last row/column.
fn tile_nw_corner(tile: TileId) -> (f64, f64) {
    let n = f64::from(TileId::tiles_per_axis(tile.z));
    let lon = f64::from(tile.x) / n * 360.0 - 180.0;
    let lat = (PI * (1.0 - 2.0 * f64::from(tile.y) / n))
        .sinh()
        .atan()
        .to_degrees();
    (lon, lat)
}

fn wrap_lon(lon_deg: f64) -> f64 {
    let mut lon = (lon_deg + 180.0) % 360.0;
    if lon < 0.0 {
        lon += 360.0;
    }
    lon - 180.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_zero_is_one_world_tile() {
        assert_eq!(tile_for_lon_lat(0.0, 0.0, 0), TileId { z: 0, x: 0, y: 0 });
        assert_eq!(
            tile_for_lon_lat(-179.9, 84.0, 0),
            TileId { z: 0, x: 0, y: 0 }
        );
        assert_eq!(
            tile_for_lon_lat(179.9, -84.0, 0),
            TileId { z: 0, x: 0, y: 0 }
        );

        let bounds = tile_bounds_lon_lat(TileId { z: 0, x: 0, y: 0 });
        assert!((bounds[0] + 180.0).abs() < 1e-12);
        assert!((bounds[2] - 180.0).abs() < 1e-12);
        assert!((bounds[1] + MERCATOR_MAX_LAT_DEG).abs() < 1e-9);
        assert!((bounds[3] - MERCATOR_MAX_LAT_DEG).abs() < 1e-9);
    }

    #[test]
    fn everest_lands_in_the_known_z10_tile() {
        // Anchor value cross-checked against the OSM slippy-map tile calculator.
        let tile = tile_for_lon_lat(86.925, 27.9881, 10);
        assert_eq!(
            tile,
            TileId {
                z: 10,
                x: 759,
                y: 429
            }
        );

        let bounds = tile_bounds_lon_lat(tile);
        assert!(bounds[0] <= 86.925 && 86.925 < bounds[2]);
        assert!(bounds[1] <= 27.9881 && 27.9881 < bounds[3]);
    }

    #[test]
    fn tile_centers_round_trip_across_zooms_and_hemispheres() {
        for &(lon, lat) in &[
            (0.1, 0.1),
            (-122.4194, 37.7749),
            (151.2153, -33.8568),
            (179.5, 71.0),
            (-179.5, -55.0),
        ] {
            for z in [1u8, 4, 8, 12, 16] {
                let tile = tile_for_lon_lat(lon, lat, z);
                let b = tile_bounds_lon_lat(tile);
                let (clon, clat) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
                assert_eq!(
                    tile_for_lon_lat(clon, clat, z),
                    tile,
                    "center of {tile:?} must map back to it"
                );
            }
        }
    }

    #[test]
    fn parent_child_relationships_are_consistent() {
        let tile = TileId {
            z: 10,
            x: 759,
            y: 429,
        };
        for child in tile.children() {
            assert_eq!(child.parent(), Some(tile));
        }
        assert_eq!(TileId { z: 0, x: 0, y: 0 }.parent(), None);
    }

    #[test]
    fn out_of_range_inputs_clamp_and_wrap() {
        // Poles clamp into the last Mercator row instead of overflowing the index.
        let north = tile_for_lon_lat(0.0, 90.0, 4);
        assert_eq!(north.y, 0);
        let south = tile_for_lon_lat(0.0, -90.0, 4);
        assert_eq!(south.y, TileId::tiles_per_axis(4) - 1);

        // Longitudes wrap: 190°E is 170°W.
        assert_eq!(
            tile_for_lon_lat(190.0, 0.0, 6),
            tile_for_lon_lat(-170.0, 0.0, 6)
        );
    }
}
