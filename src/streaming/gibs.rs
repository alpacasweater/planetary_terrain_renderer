use crate::{
    math::{Coordinate, TerrainShape},
    streaming::{
        CacheTileEncoding, CachedTileMetadata, MaterializedStreamingTile, StreamedAttachmentKind,
        StreamingProviderError, StreamingSourceAvailability, StreamingSourceDescriptor,
        StreamingSourceKind, StreamingTileProvider, StreamingTileRequest,
    },
};
use bevy::{math::DVec2, prelude::Resource};
use image::{DynamicImage, ImageFormat};
use std::{
    io::{Cursor, Read},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiff::encoder::{TiffEncoder, colortype};

const DEFAULT_GIBS_WMS_ENDPOINT: &str = "https://gibs.earthdata.nasa.gov/wms/epsg4326/best/wms.cgi";
const DEFAULT_GIBS_TRUE_COLOR_LAYER: &str = "MODIS_Terra_CorrectedReflectance_TrueColor";
const DEFAULT_GIBS_IMAGE_FORMAT: &str = "image/png";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NasaGibsImageryConfig {
    pub source_id: String,
    pub wms_endpoint: String,
    pub layer: String,
    pub image_format: String,
    pub time: Option<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl Default for NasaGibsImageryConfig {
    fn default() -> Self {
        Self {
            source_id: "nasa_gibs/modis_true_color".to_string(),
            wms_endpoint: DEFAULT_GIBS_WMS_ENDPOINT.to_string(),
            layer: DEFAULT_GIBS_TRUE_COLOR_LAYER.to_string(),
            image_format: DEFAULT_GIBS_IMAGE_FORMAT.to_string(),
            time: None,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
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
            concat!(
                "{}?service=WMS&request=GetMap&version=1.1.1",
                "&layers={}&styles=&srs=EPSG:4326",
                "&bbox={:.10},{:.10},{:.10},{:.10}",
                "&width={}&height={}&format={}&transparent=FALSE"
            ),
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

#[derive(Clone, Debug, Resource)]
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
        validate_request(request)?;
        let bbox_lon_lat = request_lon_lat_bbox(request)?;

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
    ) -> Result<MaterializedStreamingTile, StreamingProviderError> {
        let planned = self.plan_get_map(request)?;
        let url = planned.url();

        let agent = ureq::AgentBuilder::new()
            .timeout_connect(self.config.connect_timeout)
            .timeout_read(self.config.read_timeout)
            .timeout_write(self.config.read_timeout)
            .build();

        let response = agent.get(&url).call().map_err(map_ureq_error)?;

        let content_type = response
            .header("content-type")
            .map(|value| value.split(';').next().unwrap_or(value).trim().to_string())
            .unwrap_or_else(|| planned.image_format.clone());

        let mut body = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|error| {
                StreamingProviderError::Transient(format!("gibs read failed: {error}"))
            })?;

        let source_format = ImageFormat::from_mime_type(&content_type)
            .or_else(|| ImageFormat::from_mime_type(&planned.image_format))
            .ok_or_else(|| {
                StreamingProviderError::Permanent(format!(
                    "unsupported GIBS response content type '{content_type}'"
                ))
            })?;

        let source_image =
            image::load_from_memory_with_format(&body, source_format).map_err(|error| {
                StreamingProviderError::Permanent(format!("gibs image decode failed: {error}"))
            })?;

        let target_rgb = remap_source_to_tile(&source_image, request, planned.bbox_lon_lat)?;
        let encoded_tile = encode_rgb_tiff(
            request.attachment_config.texture_size,
            request.attachment_config.texture_size,
            &target_rgb,
        )?;
        let fetch_time_ms = current_unix_ms();

        Ok(MaterializedStreamingTile {
            bytes: encoded_tile,
            metadata: CachedTileMetadata {
                format_version: crate::streaming::CURRENT_STREAMING_CACHE_FORMAT_VERSION,
                terrain_path: request.terrain_path.clone(),
                attachment_label: request.attachment_label.clone(),
                coordinate: request.coordinate,
                source: self.descriptor(),
                fetched_at_unix_ms: fetch_time_ms,
                expires_at_unix_ms: None,
                source_zoom: None,
                source_revision: self.config.time.clone(),
                source_content_hash: None,
                source_crs: Some("EPSG:4326".to_string()),
                encoding: CacheTileEncoding::Tiff,
            },
        })
    }
}

fn validate_request(request: &StreamingTileRequest) -> Result<(), StreamingProviderError> {
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

    Ok(())
}

fn request_lon_lat_bbox(
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

fn texture_sample_coordinate(
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

fn remap_source_to_tile(
    source_image: &DynamicImage,
    request: &StreamingTileRequest,
    bbox_lon_lat: [f64; 4],
) -> Result<Vec<u8>, StreamingProviderError> {
    let source_image = source_image.to_rgb8();
    let width = request.attachment_config.texture_size;
    let height = request.attachment_config.texture_size;
    let bbox_center_lon = 0.5 * (bbox_lon_lat[0] + bbox_lon_lat[2]);
    let source_width = source_image.width();
    let source_height = source_image.height();

    if source_width == 0 || source_height == 0 {
        return Err(StreamingProviderError::Permanent(
            "gibs response image had zero dimensions".to_string(),
        ));
    }

    let mut remapped = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let (lat_deg, lon_deg) =
                texture_sample_coordinate(request, x as f64, y as f64).lat_lon_degrees();
            let lon_deg = normalize_lon_around(lon_deg, bbox_center_lon);

            let u = if (bbox_lon_lat[2] - bbox_lon_lat[0]).abs() < f64::EPSILON {
                0.5
            } else {
                (lon_deg - bbox_lon_lat[0]) / (bbox_lon_lat[2] - bbox_lon_lat[0])
            };
            let v = if (bbox_lon_lat[3] - bbox_lon_lat[1]).abs() < f64::EPSILON {
                0.5
            } else {
                (bbox_lon_lat[3] - lat_deg) / (bbox_lon_lat[3] - bbox_lon_lat[1])
            };

            let sample = bilinear_sample_rgb8(&source_image, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
            remapped.extend_from_slice(&sample);
        }
    }

    Ok(remapped)
}

fn bilinear_sample_rgb8(image: &image::RgbImage, u: f64, v: f64) -> [u8; 3] {
    let width = image.width().saturating_sub(1) as f64;
    let height = image.height().saturating_sub(1) as f64;
    let sample_x = u * width;
    let sample_y = v * height;

    let x0 = sample_x.floor() as u32;
    let y0 = sample_y.floor() as u32;
    let x1 = (x0 + 1).min(image.width().saturating_sub(1));
    let y1 = (y0 + 1).min(image.height().saturating_sub(1));
    let tx = sample_x.fract();
    let ty = sample_y.fract();

    let top_left = image.get_pixel(x0, y0).0.map(f64::from);
    let top_right = image.get_pixel(x1, y0).0.map(f64::from);
    let bottom_left = image.get_pixel(x0, y1).0.map(f64::from);
    let bottom_right = image.get_pixel(x1, y1).0.map(f64::from);

    let mut output = [0_u8; 3];
    for channel in 0..3 {
        let top = top_left[channel] * (1.0 - tx) + top_right[channel] * tx;
        let bottom = bottom_left[channel] * (1.0 - tx) + bottom_right[channel] * tx;
        let value = top * (1.0 - ty) + bottom * ty;
        output[channel] = value.round().clamp(0.0, 255.0) as u8;
    }
    output
}

fn encode_rgb_tiff(
    width: u32,
    height: u32,
    rgb_bytes: &[u8],
) -> Result<Vec<u8>, StreamingProviderError> {
    let mut cursor = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut cursor).map_err(|error| {
        StreamingProviderError::Permanent(format!("failed to create TIFF encoder: {error}"))
    })?;
    encoder
        .write_image::<colortype::RGB8>(width, height, rgb_bytes)
        .map_err(|error| {
            StreamingProviderError::Permanent(format!("failed to encode TIFF tile: {error}"))
        })?;
    Ok(cursor.into_inner())
}

fn map_ureq_error(error: ureq::Error) -> StreamingProviderError {
    match error {
        ureq::Error::Status(code, response) => {
            let status = format!("gibs returned HTTP {code} for {}", response.get_url());
            if code == 429 || code >= 500 {
                StreamingProviderError::Transient(status)
            } else {
                StreamingProviderError::Permanent(status)
            }
        }
        ureq::Error::Transport(error) => StreamingProviderError::Transient(format!(
            "gibs transport failed: {}",
            error.message().unwrap_or("transport error without message")
        )),
    }
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
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
        math::{TerrainShape, TileCoordinate},
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
                0,
                2,
                IVec2::new(1, 1),
            )))
            .expect("mid-latitude tile should be plannable");

        let url = planned.url();
        assert!(url.starts_with(DEFAULT_GIBS_WMS_ENDPOINT));
        assert!(url.contains("service=WMS"));
        assert!(url.contains("request=GetMap"));
        assert!(url.contains("version=1.1.1"));
        assert!(url.contains("layers=MODIS_Terra_CorrectedReflectance_TrueColor"));
        assert!(url.contains("format=image/png"));
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

    #[test]
    fn texture_sample_coordinate_handles_border_pixels() {
        let request = imagery_request(TileCoordinate::new(2, 2, IVec2::new(1, 1)));
        let coordinate = texture_sample_coordinate(&request, 0.0, 0.0);
        let (lat_deg, lon_deg) = coordinate.lat_lon_degrees();
        assert!(lat_deg.is_finite());
        assert!(lon_deg.is_finite());
    }
}
