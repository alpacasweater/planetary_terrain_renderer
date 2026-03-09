use crate::{
    math::Coordinate,
    streaming::{StreamingProviderError, StreamingTileRequest},
};
use bevy::math::DVec2;

pub(crate) fn request_lon_lat_bbox(
    request: &StreamingTileRequest,
) -> Result<[f64; 4], StreamingProviderError> {
    let samples = sample_request_lon_lat(
        request,
        &[
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (0.0, 0.5),
            (0.5, 0.5),
            (1.0, 0.5),
            (0.0, 1.0),
            (0.5, 1.0),
            (1.0, 1.0),
        ],
    );

    bounded_lon_lat_bbox(samples)
}

fn sample_request_lon_lat(
    request: &StreamingTileRequest,
    normalized_points: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    let last_pixel = request.attachment_config.texture_size.saturating_sub(1) as f64;
    normalized_points
        .iter()
        .map(|(u, v)| {
            texture_sample_coordinate(request, u * last_pixel, v * last_pixel).lat_lon_degrees()
        })
        .collect()
}

pub(crate) fn texture_sample_coordinate(
    request: &StreamingTileRequest,
    pixel_x: f64,
    pixel_y: f64,
) -> Coordinate {
    let center_size = request.attachment_config.center_size() as f64;
    let border_size = request.attachment_config.border_size as f64;
    let tile_count = (1_u32 << request.coordinate.lod) as f64;
    let sample_uv = DVec2::new(
        (request.coordinate.xy.x as f64 + ((pixel_x - border_size + 0.5) / center_size))
            / tile_count,
        (request.coordinate.xy.y as f64 + ((pixel_y - border_size + 0.5) / center_size))
            / tile_count,
    );
    let raw = Coordinate::new(request.coordinate.face, sample_uv);
    Coordinate::from_unit_position(
        raw.unit_position(request.terrain_shape.is_spherical()),
        request.terrain_shape.is_spherical(),
    )
}

fn bounded_lon_lat_bbox(mut samples: Vec<(f64, f64)>) -> Result<[f64; 4], StreamingProviderError> {
    let center_lon = samples[samples.len() / 2].1;
    for (_, lon_deg) in &mut samples {
        *lon_deg = normalize_lon_around(*lon_deg, center_lon);
    }

    let (min_lat, max_lat) = samples.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(min_lat, max_lat), (lat_deg, _)| (min_lat.min(*lat_deg), max_lat.max(*lat_deg)),
    );
    let (min_lon, max_lon) = samples.iter().fold(
        (f64::INFINITY, f64::NEG_INFINITY),
        |(min_lon, max_lon), (_, lon_deg)| (min_lon.min(*lon_deg), max_lon.max(*lon_deg)),
    );

    if max_lon - min_lon > 180.0 {
        return Err(StreamingProviderError::Unsupported(
            "tile longitude span crosses the antimeridian; split-request planning is not implemented yet".to_string(),
        ));
    }

    Ok([
        normalize_lon_to_180(min_lon),
        min_lat.clamp(-90.0, 90.0),
        normalize_lon_to_180(max_lon),
        max_lat.clamp(-90.0, 90.0),
    ])
}

pub(crate) fn normalize_lon_around(lon_deg: f64, reference_deg: f64) -> f64 {
    let mut lon = lon_deg;
    while lon - reference_deg > 180.0 {
        lon -= 360.0;
    }
    while lon - reference_deg < -180.0 {
        lon += 360.0;
    }
    lon
}

pub(crate) fn normalize_lon_to_180(lon_deg: f64) -> f64 {
    let mut lon = lon_deg % 360.0;
    if lon > 180.0 {
        lon -= 360.0;
    } else if lon <= -180.0 {
        lon += 360.0;
    }
    lon
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::{TerrainShape, TileCoordinate},
        terrain_data::{AttachmentConfig, AttachmentFormat, AttachmentLabel},
    };
    use bevy::math::IVec2;

    fn height_request(tile: TileCoordinate) -> StreamingTileRequest {
        StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Height,
            attachment_config: AttachmentConfig {
                texture_size: 128,
                border_size: 4,
                mip_level_count: 4,
                mask: false,
                format: AttachmentFormat::R32F,
            },
            coordinate: tile,
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 3,
        }
    }

    #[test]
    fn texture_sample_coordinate_handles_border_pixels() {
        let request = height_request(TileCoordinate::new(2, 2, IVec2::new(1, 1)));
        let coordinate = texture_sample_coordinate(&request, 0.0, 0.0);
        let (lat_deg, lon_deg) = coordinate.lat_lon_degrees();
        assert!(lat_deg.is_finite());
        assert!(lon_deg.is_finite());
    }

    #[test]
    fn longitude_normalization_stays_close_to_reference() {
        assert!((normalize_lon_around(-179.0, 179.0) - 181.0).abs() < 1e-9);
        assert!((normalize_lon_around(179.0, -179.0) + 181.0).abs() < 1e-9);
        assert!((normalize_lon_to_180(181.0) + 179.0).abs() < 1e-9);
    }
}
