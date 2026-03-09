use crate::{
    math::{Coordinate, TerrainShape, TileCoordinate},
    streaming::{
        StreamedAttachmentKind, StreamingProviderError, StreamingSourceAvailability,
        StreamingSourceDescriptor, StreamingSourceKind, StreamingTileProvider,
        StreamingTileRequest,
    },
};
use bevy::math::DVec2;

const DEFAULT_GIBS_WMS_ENDPOINT: &str = "https://gibs.earthdata.nasa.gov/wms/epsg4326/best/wms.cgi";
const DEFAULT_GIBS_TRUE_COLOR_LAYER: &str = "MODIS_Terra_CorrectedReflectance_TrueColor";
const DEFAULT_GIBS_IMAGE_FORMAT: &str = "image/tiff";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NasaGibsImageryConfig {
    pub source_id: String,
    pub wms_endpoint: String,
    pub layer: String,
    pub image_format: String,
    pub time: Option<String>,
}

impl Default for NasaGibsImageryConfig {
    fn default() -> Self {
        Self {
            source_id: "nasa_gibs/modis_true_color".to_string(),
            wms_endpoint: DEFAULT_GIBS_WMS_ENDPOINT.to_string(),
            layer: DEFAULT_GIBS_TRUE_COLOR_LAYER.to_string(),
            image_format: DEFAULT_GIBS_IMAGE_FORMAT.to_string(),
            time: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GibsGetMapRequest {
    pub endpoint: String,
    pub layer: String,
    pub image_format: String,
    pub width: u32,
    pub height: u32,
    pub bbox_lon_lat: [f64; 4],
    pub time: Option<String>,
}

impl GibsGetMapRequest {
    pub fn url(&self) -> String {
        let mut url = format!(
            "{}?service=WMS&request=GetMap&version=1.1.1&layers={}&styles=&srs=EPSG:4326&bbox={:.10},{:.10},{:.10},{:.10}&width={}&height={}&format={}&transparent=FALSE",
            self.endpoint,
            self.layer,
            self.bbox_lon_lat[0],
            self.bbox_lon_lat[1],
            self.bbox_lon_lat[2],
            self.bbox_lon_lat[3],
            self.width,
            self.height,
            self.image_format
        );

        if let Some(time) = &self.time {
            url.push_str("&time=");
            url.push_str(time);
        }

        url
    }
}

#[derive(Clone, Debug)]
pub struct NasaGibsImageryProvider {
    config: NasaGibsImageryConfig,
}

impl Default for NasaGibsImageryProvider {
    fn default() -> Self {
        Self {
            config: NasaGibsImageryConfig::default(),
        }
    }
}

impl NasaGibsImageryProvider {
    pub fn new(config: NasaGibsImageryConfig) -> Self {
        Self { config }
    }

    pub fn plan_get_map(
        &self,
        request: &StreamingTileRequest,
    ) -> Result<GibsGetMapRequest, StreamingProviderError> {
        if request.attachment_kind() != StreamedAttachmentKind::Imagery {
            return Err(StreamingProviderError::Unsupported(
                "NASA GIBS only serves imagery attachments".to_string(),
            ));
        }

        if !matches!(
            request.terrain_shape,
            TerrainShape::Sphere { .. } | TerrainShape::Spheroid { .. }
        ) {
            return Err(StreamingProviderError::Unsupported(
                "NASA GIBS provider currently requires a spherical terrain".to_string(),
            ));
        }

        let bbox_lon_lat = tile_lon_lat_bbox(request.coordinate)?;

        Ok(GibsGetMapRequest {
            endpoint: self.config.wms_endpoint.clone(),
            layer: self.config.layer.clone(),
            image_format: self.config.image_format.clone(),
            width: request.attachment_config.texture_size,
            height: request.attachment_config.texture_size,
            bbox_lon_lat,
            time: self.config.time.clone(),
        })
    }
}

impl StreamingTileProvider for NasaGibsImageryProvider {
    fn descriptor(&self) -> StreamingSourceDescriptor {
        StreamingSourceDescriptor {
            source_id: self.config.source_id.clone(),
            source_kind: StreamingSourceKind::NasaGibs,
            attachment_kind: StreamedAttachmentKind::Imagery,
        }
    }

    fn availability(&self, request: &StreamingTileRequest) -> StreamingSourceAvailability {
        match self.plan_get_map(request) {
            Ok(_) => StreamingSourceAvailability::Available,
            Err(StreamingProviderError::Unsupported(reason))
            | Err(StreamingProviderError::Unavailable(reason)) => {
                StreamingSourceAvailability::Unavailable { reason }
            }
            Err(StreamingProviderError::Transient(reason))
            | Err(StreamingProviderError::Permanent(reason)) => {
                StreamingSourceAvailability::Unavailable { reason }
            }
        }
    }

    fn materialize_tile(
        &self,
        request: &StreamingTileRequest,
    ) -> Result<crate::streaming::MaterializedStreamingTile, StreamingProviderError> {
        let _ = self.plan_get_map(request)?;
        Err(StreamingProviderError::Unsupported(
            "NASA GIBS cache materialization is not implemented yet; this provider currently plans WMS requests only".to_string(),
        ))
    }
}

fn tile_lon_lat_bbox(tile: TileCoordinate) -> Result<[f64; 4], StreamingProviderError> {
    let tile_count = (1_u32 << tile.lod) as f64;
    let tile_origin = tile.xy.as_dvec2() / tile_count;
    let tile_size = DVec2::splat(1.0 / tile_count);
    let sample_uvs = [
        DVec2::new(0.0, 0.0),
        DVec2::new(1.0, 0.0),
        DVec2::new(0.0, 1.0),
        DVec2::new(1.0, 1.0),
        DVec2::new(0.5, 0.5),
    ];

    let mut samples = sample_uvs
        .into_iter()
        .map(|offset| {
            Coordinate::new(tile.face, tile_origin + offset * tile_size).lat_lon_degrees()
        })
        .collect::<Vec<_>>();

    let center_lon = samples[4].1;
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

fn normalize_lon_around(lon_deg: f64, reference_deg: f64) -> f64 {
    let mut lon = lon_deg;
    while lon - reference_deg > 180.0 {
        lon -= 360.0;
    }
    while lon - reference_deg < -180.0 {
        lon += 360.0;
    }
    lon
}

fn normalize_lon_to_180(lon_deg: f64) -> f64 {
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
        math::TerrainShape,
        terrain_data::{AttachmentConfig, AttachmentFormat, AttachmentLabel},
    };
    use bevy::math::IVec2;

    fn imagery_request(tile: TileCoordinate) -> StreamingTileRequest {
        StreamingTileRequest {
            terrain_path: "terrains/earth".to_string(),
            attachment_label: AttachmentLabel::Custom("albedo".to_string()),
            attachment_config: AttachmentConfig {
                texture_size: 512,
                border_size: 4,
                mip_level_count: 4,
                mask: false,
                format: AttachmentFormat::Rgb8U,
            },
            coordinate: tile,
            terrain_shape: TerrainShape::WGS84,
            terrain_lod_count: 5,
        }
    }

    #[test]
    fn provider_plans_official_gibs_wms_requests() {
        let provider = NasaGibsImageryProvider::default();
        let planned = provider
            .plan_get_map(&imagery_request(TileCoordinate::new(
                2,
                1,
                IVec2::new(0, 0),
            )))
            .expect("mid-latitude tile should be plannable");

        let url = planned.url();
        assert!(url.starts_with(DEFAULT_GIBS_WMS_ENDPOINT));
        assert!(url.contains("service=WMS"));
        assert!(url.contains("request=GetMap"));
        assert!(url.contains("version=1.1.1"));
        assert!(url.contains("layers=MODIS_Terra_CorrectedReflectance_TrueColor"));
        assert!(url.contains("format=image/tiff"));
        assert!(url.contains("srs=EPSG:4326"));
        assert!(url.contains("width=512"));
        assert!(url.contains("height=512"));
    }

    #[test]
    fn provider_rejects_height_requests() {
        let provider = NasaGibsImageryProvider::default();
        let mut request = imagery_request(TileCoordinate::new(0, 1, IVec2::new(0, 0)));
        request.attachment_label = AttachmentLabel::Height;

        assert!(matches!(
            provider.availability(&request),
            StreamingSourceAvailability::Unavailable { .. }
        ));
    }

    #[test]
    fn longitude_normalization_stays_close_to_reference() {
        assert!((normalize_lon_around(-179.0, 179.0) - 181.0).abs() < 1e-9);
        assert!((normalize_lon_around(179.0, -179.0) + 181.0).abs() < 1e-9);
        assert!((normalize_lon_to_180(181.0) + 179.0).abs() < 1e-9);
    }
}
