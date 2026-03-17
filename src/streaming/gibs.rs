use crate::{
    math::TerrainShape,
    streaming::{
        CacheTileEncoding, CachedTileMetadata, MaterializedStreamingTile, StreamedAttachmentKind,
        StreamingProviderError, StreamingSourceAvailability, StreamingSourceDescriptor,
        StreamingSourceKind, StreamingTileProvider, StreamingTileRequest,
        terrain_sampling::{normalize_lon_around, texture_sample_coordinate},
    },
};
use bevy::prelude::Resource;
use image::{DynamicImage, ImageFormat};
use std::{
    io::{Cursor, Read},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiff::encoder::{TiffEncoder, colortype};

const DEFAULT_GIBS_WMS_ENDPOINT: &str = "https://gibs.earthdata.nasa.gov/wms/epsg4326/best/wms.cgi";
const DEFAULT_GIBS_TRUE_COLOR_LAYER: &str = "MODIS_Terra_CorrectedReflectance_TrueColor";
const DEFAULT_GIBS_IMAGE_FORMAT: &str = "image/png";
const EOX_WMS_ENDPOINT: &str = "https://tiles.maps.eox.at/wms";
const EOX_S2CLOUDLESS_2017_LAYER: &str = "s2cloudless-2017";
const EOX_IMAGE_FORMAT: &str = "image/jpeg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NasaGibsImageryConfig {
    pub source_id: String,
    pub source_kind: StreamingSourceKind,
    pub wms_endpoint: String,
    pub layer: String,
    pub image_format: String,
    pub time: Option<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
    pub fallback: Option<Box<NasaGibsImageryConfig>>,
}

impl Default for NasaGibsImageryConfig {
    fn default() -> Self {
        Self::gibs_modis_true_color()
    }
}

impl NasaGibsImageryConfig {
    pub fn gibs_modis_true_color() -> Self {
        Self {
            source_id: "nasa_gibs/modis_true_color".to_string(),
            source_kind: StreamingSourceKind::NasaGibs,
            wms_endpoint: DEFAULT_GIBS_WMS_ENDPOINT.to_string(),
            layer: DEFAULT_GIBS_TRUE_COLOR_LAYER.to_string(),
            image_format: DEFAULT_GIBS_IMAGE_FORMAT.to_string(),
            time: None,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            fallback: None,
        }
    }

    pub fn eox_s2cloudless_2017() -> Self {
        Self {
            source_id: "eox/s2cloudless_2017".to_string(),
            source_kind: StreamingSourceKind::Custom("eox_cloudless".to_string()),
            wms_endpoint: EOX_WMS_ENDPOINT.to_string(),
            layer: EOX_S2CLOUDLESS_2017_LAYER.to_string(),
            image_format: EOX_IMAGE_FORMAT.to_string(),
            time: None,
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(30),
            fallback: Some(Box::new(Self::gibs_modis_true_color())),
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
        Self::plan_get_map_with_config(&self.config, request)
    }

    fn plan_get_map_with_config(
        config: &NasaGibsImageryConfig,
        request: &StreamingTileRequest,
    ) -> Result<GibsGetMapRequest, StreamingProviderError> {
        validate_request(request)?;
        let bbox_lon_lat = request_lon_lat_bbox(request)?;

        Ok(GibsGetMapRequest {
            endpoint: config.wms_endpoint.clone(),
            layer: config.layer.clone(),
            image_format: config.image_format.clone(),
            width: request.attachment_config.texture_size,
            height: request.attachment_config.texture_size,
            bbox_lon_lat,
            time: config.time.clone(),
        })
    }
}

fn descriptor_from_config(config: &NasaGibsImageryConfig) -> StreamingSourceDescriptor {
    StreamingSourceDescriptor {
        source_id: config.source_id.clone(),
        source_kind: config.source_kind.clone(),
        attachment_kind: StreamedAttachmentKind::Imagery,
    }
}

impl StreamingTileProvider for NasaGibsImageryProvider {
    fn descriptor(&self) -> StreamingSourceDescriptor {
        descriptor_from_config(&self.config)
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
        materialize_tile_with_config(&self.config, request)
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
    crate::streaming::terrain_sampling::request_lon_lat_bbox(request)
}

fn materialize_tile_with_config(
    config: &NasaGibsImageryConfig,
    request: &StreamingTileRequest,
) -> Result<MaterializedStreamingTile, StreamingProviderError> {
    let planned = NasaGibsImageryProvider::plan_get_map_with_config(config, request)?;
    let url = planned.url();

    let agent = ureq::AgentBuilder::new()
        .timeout_connect(config.connect_timeout)
        .timeout_read(config.read_timeout)
        .timeout_write(config.read_timeout)
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
        .map_err(|error| StreamingProviderError::Transient(format!("gibs read failed: {error}")))?;

    let source_image = match decode_source_image(&body, &content_type, &planned.image_format) {
        Ok(image) => image,
        Err(error) => {
            if let Some(fallback) = &config.fallback {
                bevy::log::warn!(
                    "Imagery source {} failed for {:?}: {}. Retrying with fallback {}",
                    config.source_id,
                    request.coordinate,
                    error,
                    fallback.source_id,
                );
                return materialize_tile_with_config(fallback, request);
            }

            return Err(error);
        }
    };

    let target_rgb = remap_source_to_tile(&source_image, request, planned.bbox_lon_lat)?;
    if imagery_looks_like_blank_fill(&target_rgb) {
        if let Some(fallback) = &config.fallback {
            bevy::log::warn!(
                "Imagery source {} produced a near-blank tile for {:?}; retrying with fallback {}",
                config.source_id,
                request.coordinate,
                fallback.source_id,
            );
            return materialize_tile_with_config(fallback, request);
        }
    }

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
            source: descriptor_from_config(config),
            fetched_at_unix_ms: fetch_time_ms,
            expires_at_unix_ms: None,
            source_zoom: None,
            source_revision: config.time.clone(),
            source_content_hash: None,
            source_crs: Some("EPSG:4326".to_string()),
            encoding: CacheTileEncoding::Tiff,
        },
    })
}

fn decode_source_image(
    body: &[u8],
    content_type: &str,
    planned_image_format: &str,
) -> Result<DynamicImage, StreamingProviderError> {
    let guessed_format = image::guess_format(body).ok();
    let source_format = guessed_format
        .or_else(|| ImageFormat::from_mime_type(content_type))
        .or_else(|| ImageFormat::from_mime_type(planned_image_format))
        .ok_or_else(|| {
            StreamingProviderError::Permanent(format!(
                "unsupported imagery response content type '{content_type}'"
            ))
        })?;

    if guessed_format.is_none() && response_looks_like_error_document(body) {
        return Err(StreamingProviderError::Permanent(format!(
            "imagery source returned a non-image response (content_type='{content_type}', preview='{}')",
            response_preview(body)
        )));
    }

    image::load_from_memory_with_format(body, source_format).map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "imagery decode failed (content_type='{content_type}', format={source_format:?}): {error}"
        ))
    })
}

fn response_looks_like_error_document(body: &[u8]) -> bool {
    let prefix = body
        .iter()
        .copied()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take(96)
        .collect::<Vec<_>>();
    let prefix = String::from_utf8_lossy(&prefix).to_ascii_lowercase();

    prefix.starts_with("<!doctype")
        || prefix.starts_with("<html")
        || prefix.starts_with("<?xml")
        || prefix.starts_with("<serviceexceptionreport")
        || prefix.starts_with("<ows:exceptionreport")
}

fn response_preview(body: &[u8]) -> String {
    String::from_utf8_lossy(&body[..body.len().min(160)])
        .replace('\n', " ")
        .replace('\r', " ")
        .chars()
        .take(160)
        .collect()
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

fn imagery_looks_like_blank_fill(rgb_bytes: &[u8]) -> bool {
    if rgb_bytes.is_empty() {
        return true;
    }

    let mut min_value = u8::MAX;
    let mut max_value = u8::MIN;
    let mut sum = 0_u64;

    for &value in rgb_bytes {
        min_value = min_value.min(value);
        max_value = max_value.max(value);
        sum += value as u64;
    }

    let mean_value = sum as f64 / rgb_bytes.len() as f64;
    max_value.saturating_sub(min_value) <= 6 && !(20.0..=235.0).contains(&mean_value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::{TerrainShape, TileCoordinate},
        streaming::StreamingRequestPriority,
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
            priority: StreamingRequestPriority::Background,
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
    fn blank_fill_detection_flags_near_uniform_extremes() {
        assert!(imagery_looks_like_blank_fill(&[240; 128 * 128 * 3]));
        assert!(imagery_looks_like_blank_fill(&[5; 128 * 128 * 3]));

        let mut textured = vec![140_u8; 128 * 128 * 3];
        textured[0] = 10;
        textured[1] = 200;
        assert!(!imagery_looks_like_blank_fill(&textured));
    }

    #[test]
    fn eox_config_includes_modis_fallback() {
        let config = NasaGibsImageryConfig::eox_s2cloudless_2017();
        let fallback = config.fallback.expect("EOX preset should install fallback");
        assert_eq!(fallback.source_id, "nasa_gibs/modis_true_color");
    }

    #[test]
    fn html_response_is_reported_as_non_image() {
        let error = decode_source_image(
            b"<!DOCTYPE html><html><body>blocked</body></html>",
            "text/html",
            "image/png",
        )
        .expect_err("HTML bodies should be rejected before PNG decode");

        let message = error.to_string();
        assert!(message.contains("non-image response"));
        assert!(message.contains("text/html"));
    }
}
