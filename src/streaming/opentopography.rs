use crate::{
    math::TerrainShape,
    streaming::{
        CacheTileEncoding, CachedTileMetadata, MaterializedStreamingTile, StreamedAttachmentKind,
        StreamingProviderError, StreamingSourceAvailability, StreamingSourceDescriptor,
        StreamingSourceKind, StreamingTileProvider, StreamingTileRequest,
        terrain_sampling::{normalize_lon_around, request_lon_lat_bbox, texture_sample_coordinate},
    },
    terrain_data::AttachmentFormat,
};
use bevy::prelude::Resource;
use std::{
    env,
    io::{Cursor, Read},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tiff::{
    decoder::{Decoder, DecodingResult, Limits},
    encoder::{TiffEncoder, colortype},
};

const DEFAULT_OPENTOPOGRAPHY_ENDPOINT: &str = "https://portal.opentopography.org/API/globaldem";
const DEFAULT_OPENTOPOGRAPHY_DEM_TYPE: &str = "AW3D30_E";
const DEFAULT_OPENTOPOGRAPHY_OUTPUT_FORMAT: &str = "GTiff";
const EARTH_RADIUS_KM: f64 = 6_371.0;
const MAX_REQUEST_AREA_30M_SQ_KM: f64 = 450_000.0;
const MAX_REQUEST_AREA_90M_SQ_KM: f64 = 4_050_000.0;
const PLAUSIBLE_EARTH_MIN_HEIGHT_M: f32 = -20_000.0;
const PLAUSIBLE_EARTH_MAX_HEIGHT_M: f32 = 20_000.0;
const OPENTOPOGRAPHY_MAX_DECODING_BUFFER_BYTES: usize = 1024 * 1024 * 1024;
const OPENTOPOGRAPHY_MAX_INTERMEDIATE_BUFFER_BYTES: usize = 512 * 1024 * 1024;
const OPENTOPOGRAPHY_MAX_IFD_VALUE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenTopographyHeightConfig {
    pub source_id: String,
    pub endpoint: String,
    pub dem_type: String,
    pub output_format: String,
    pub api_key: Option<String>,
    pub connect_timeout: Duration,
    pub read_timeout: Duration,
}

impl Default for OpenTopographyHeightConfig {
    fn default() -> Self {
        Self {
            source_id: "opentopography/aw3d30_e".to_string(),
            endpoint: DEFAULT_OPENTOPOGRAPHY_ENDPOINT.to_string(),
            dem_type: env::var("OPENTOPOGRAPHY_DEM_TYPE")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_OPENTOPOGRAPHY_DEM_TYPE.to_string()),
            output_format: DEFAULT_OPENTOPOGRAPHY_OUTPUT_FORMAT.to_string(),
            api_key: env::var("OPENTOPOGRAPHY_API_KEY")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    env::var("OPEN_TOPOGRAPHY_API_KEY")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                }),
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(45),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpenTopographyGlobalDemRequest {
    pub endpoint: String,
    pub dem_type: String,
    pub output_format: String,
    pub bbox_lon_lat: [f64; 4],
    pub api_key: String,
}

impl OpenTopographyGlobalDemRequest {
    pub fn url(&self) -> String {
        format!(
            concat!(
                "{}?demtype={}&south={:.10}&north={:.10}",
                "&west={:.10}&east={:.10}&outputFormat={}&API_Key={}"
            ),
            self.endpoint,
            self.dem_type,
            self.bbox_lon_lat[1],
            self.bbox_lon_lat[3],
            self.bbox_lon_lat[0],
            self.bbox_lon_lat[2],
            self.output_format,
            self.api_key
        )
    }
}

#[derive(Clone, Debug, Resource)]
pub struct OpenTopographyHeightProvider {
    config: OpenTopographyHeightConfig,
}

impl Default for OpenTopographyHeightProvider {
    fn default() -> Self {
        Self {
            config: OpenTopographyHeightConfig::default(),
        }
    }
}

impl OpenTopographyHeightProvider {
    pub fn new(config: OpenTopographyHeightConfig) -> Self {
        Self { config }
    }

    pub fn plan_global_dem(
        &self,
        request: &StreamingTileRequest,
    ) -> Result<OpenTopographyGlobalDemRequest, StreamingProviderError> {
        validate_request(request)?;

        let api_key = self.config.api_key.clone().ok_or_else(|| {
            StreamingProviderError::Unavailable(
                "OpenTopography height streaming requires OPENTOPOGRAPHY_API_KEY".to_string(),
            )
        })?;

        let bbox_lon_lat = request_lon_lat_bbox(request)?;
        validate_bbox_area_for_dem_type(&self.config.dem_type, bbox_lon_lat)?;
        Ok(OpenTopographyGlobalDemRequest {
            endpoint: self.config.endpoint.clone(),
            dem_type: self.config.dem_type.clone(),
            output_format: self.config.output_format.clone(),
            bbox_lon_lat,
            api_key,
        })
    }
}

impl StreamingTileProvider for OpenTopographyHeightProvider {
    fn descriptor(&self) -> StreamingSourceDescriptor {
        StreamingSourceDescriptor {
            source_id: self.config.source_id.clone(),
            source_kind: StreamingSourceKind::OpenTopography,
            attachment_kind: StreamedAttachmentKind::Height,
        }
    }

    fn availability(&self, request: &StreamingTileRequest) -> StreamingSourceAvailability {
        match self.plan_global_dem(request) {
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
        let planned = self.plan_global_dem(request)?;
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
            .unwrap_or_else(|| "application/octet-stream".to_string());
        let mut body = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|error| {
                StreamingProviderError::Transient(format!("opentopography read failed: {error}"))
            })?;

        let source_dem = decode_dem_tiff(&body, &content_type)?;
        let target_heights = remap_source_to_tile(&source_dem, request, planned.bbox_lon_lat)?;
        let encoded_tile = encode_height_tiff(
            request.attachment_config.texture_size,
            request.attachment_config.texture_size,
            &target_heights,
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
                source_revision: Some(self.config.dem_type.clone()),
                source_content_hash: None,
                source_crs: Some("EPSG:4326".to_string()),
                encoding: CacheTileEncoding::Tiff,
            },
        })
    }
}

fn validate_request(request: &StreamingTileRequest) -> Result<(), StreamingProviderError> {
    if request.attachment_kind() != StreamedAttachmentKind::Height {
        return Err(StreamingProviderError::Unsupported(
            "OpenTopography only serves height attachments".to_string(),
        ));
    }

    if request.attachment_config.format != AttachmentFormat::R32F {
        return Err(StreamingProviderError::Unsupported(
            "OpenTopography height streaming currently requires R32F terrain attachments"
                .to_string(),
        ));
    }

    if !matches!(
        request.terrain_shape,
        TerrainShape::Sphere { .. } | TerrainShape::Spheroid { .. }
    ) {
        return Err(StreamingProviderError::Unsupported(
            "OpenTopography provider currently requires a spherical terrain".to_string(),
        ));
    }

    Ok(())
}

fn validate_bbox_area_for_dem_type(
    dem_type: &str,
    bbox_lon_lat: [f64; 4],
) -> Result<(), StreamingProviderError> {
    let Some(max_area_sq_km) = max_request_area_sq_km_for_dem_type(dem_type) else {
        return Ok(());
    };

    let area_sq_km = bbox_area_sq_km(bbox_lon_lat);
    if area_sq_km > max_area_sq_km {
        return Err(StreamingProviderError::Unsupported(format!(
            "OpenTopography {dem_type} requests are limited to {:.0} km^2, but this tile covers about {:.0} km^2. Coarse tiles must fall back to local height until the view requests smaller regions.",
            max_area_sq_km, area_sq_km,
        )));
    }

    Ok(())
}

fn max_request_area_sq_km_for_dem_type(dem_type: &str) -> Option<f64> {
    match dem_type.trim().to_ascii_uppercase().as_str() {
        "AW3D30" | "AW3D30_E" | "SRTMGL1" | "SRTMGL1_E" | "NASADEM" | "COP30" | "EU_DTM" => {
            Some(MAX_REQUEST_AREA_30M_SQ_KM)
        }
        "SRTMGL3" | "COP90" => Some(MAX_REQUEST_AREA_90M_SQ_KM),
        _ => None,
    }
}

fn bbox_area_sq_km(bbox_lon_lat: [f64; 4]) -> f64 {
    let lon_span_rad = (bbox_lon_lat[2] - bbox_lon_lat[0]).abs().to_radians();
    let south_sin = bbox_lon_lat[1].to_radians().sin();
    let north_sin = bbox_lon_lat[3].to_radians().sin();
    EARTH_RADIUS_KM.powi(2) * lon_span_rad * (north_sin - south_sin).abs()
}

#[derive(Debug)]
struct DecodedDem {
    width: u32,
    height: u32,
    samples: Vec<f32>,
}

fn decode_dem_tiff(bytes: &[u8], content_type: &str) -> Result<DecodedDem, StreamingProviderError> {
    decode_dem_tiff_with_limits(bytes, content_type, opentopography_tiff_limits())
}

fn decode_dem_tiff_with_limits(
    bytes: &[u8],
    content_type: &str,
    limits: Limits,
) -> Result<DecodedDem, StreamingProviderError> {
    if !body_has_tiff_signature(bytes) {
        return Err(non_tiff_response_error(bytes, content_type));
    }

    let mut decoder = Decoder::new(Cursor::new(bytes))
        .map(|decoder| decoder.with_limits(limits))
        .map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to construct OpenTopography TIFF decoder (content_type='{content_type}', magic='{}'): {error}",
            body_magic_preview(bytes),
        ))
    })?;
    let (width, height) = decoder.dimensions().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to read OpenTopography TIFF dimensions (content_type='{content_type}'): {error}"
        ))
    })?;

    let samples = match decoder.read_image().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to decode OpenTopography TIFF body (content_type='{content_type}'): {error}"
        ))
    })? {
        DecodingResult::U8(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::U16(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U32(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::U64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I8(values) => values.into_iter().map(f32::from).collect(),
        DecodingResult::I16(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I32(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::I64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::F32(values) => values,
        DecodingResult::F64(values) => values.into_iter().map(|value| value as f32).collect(),
        DecodingResult::F16(_) => {
            return Err(StreamingProviderError::Permanent(
                "OpenTopography TIFF uses unsupported F16 samples".to_string(),
            ));
        }
    };

    if samples.len() != (width * height) as usize {
        return Err(StreamingProviderError::Permanent(format!(
            "OpenTopography TIFF sample count {} did not match dimensions {}x{}",
            samples.len(),
            width,
            height
        )));
    }

    if samples.iter().any(|value| {
        !value.is_finite()
            || *value < PLAUSIBLE_EARTH_MIN_HEIGHT_M
            || *value > PLAUSIBLE_EARTH_MAX_HEIGHT_M
    }) {
        return Err(StreamingProviderError::Unavailable(
            "OpenTopography DEM response contained invalid or out-of-range elevations".to_string(),
        ));
    }

    Ok(DecodedDem {
        width,
        height,
        samples,
    })
}

fn opentopography_tiff_limits() -> Limits {
    let mut limits = Limits::default();
    limits.decoding_buffer_size = OPENTOPOGRAPHY_MAX_DECODING_BUFFER_BYTES;
    limits.intermediate_buffer_size = OPENTOPOGRAPHY_MAX_INTERMEDIATE_BUFFER_BYTES;
    limits.ifd_value_size = OPENTOPOGRAPHY_MAX_IFD_VALUE_BYTES;
    limits
}

fn body_has_tiff_signature(body: &[u8]) -> bool {
    matches!(
        body.get(..4),
        Some(b"II*\0") | Some(b"MM\0*") | Some(b"II+\0") | Some(b"MM\0+")
    )
}

fn non_tiff_response_error(body: &[u8], content_type: &str) -> StreamingProviderError {
    if response_looks_like_text_document(body) || content_type_looks_textual(content_type) {
        StreamingProviderError::Permanent(format!(
            "OpenTopography returned a non-TIFF response (content_type='{content_type}', preview='{}')",
            response_preview(body)
        ))
    } else {
        StreamingProviderError::Permanent(format!(
            "OpenTopography returned a non-TIFF body (content_type='{content_type}', magic='{}')",
            body_magic_preview(body)
        ))
    }
}

fn content_type_looks_textual(content_type: &str) -> bool {
    let content_type = content_type.trim().to_ascii_lowercase();
    content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("html")
}

fn response_looks_like_text_document(body: &[u8]) -> bool {
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
        || prefix.starts_with("{\"error")
        || prefix.starts_with("{\"message")
        || prefix.starts_with("error")
        || prefix.starts_with("message")
}

fn response_preview(body: &[u8]) -> String {
    String::from_utf8_lossy(&body[..body.len().min(160)])
        .replace('\n', " ")
        .replace('\r', " ")
        .chars()
        .take(160)
        .collect()
}

fn body_magic_preview(body: &[u8]) -> String {
    let magic = body
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    if magic.is_empty() {
        "empty".to_string()
    } else {
        magic
    }
}

fn remap_source_to_tile(
    source_dem: &DecodedDem,
    request: &StreamingTileRequest,
    bbox_lon_lat: [f64; 4],
) -> Result<Vec<f32>, StreamingProviderError> {
    if source_dem.width == 0 || source_dem.height == 0 {
        return Err(StreamingProviderError::Permanent(
            "OpenTopography response raster had zero dimensions".to_string(),
        ));
    }

    let width = request.attachment_config.texture_size;
    let height = request.attachment_config.texture_size;
    let bbox_center_lon = 0.5 * (bbox_lon_lat[0] + bbox_lon_lat[2]);
    let mut remapped = Vec::with_capacity((width * height) as usize);

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

            let sample = bilinear_sample_f32(source_dem, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
            remapped.push(sample);
        }
    }

    Ok(remapped)
}

fn bilinear_sample_f32(dem: &DecodedDem, u: f64, v: f64) -> f32 {
    let width = dem.width.saturating_sub(1) as f64;
    let height = dem.height.saturating_sub(1) as f64;
    let sample_x = u * width;
    let sample_y = v * height;

    let x0 = sample_x.floor() as u32;
    let y0 = sample_y.floor() as u32;
    let x1 = (x0 + 1).min(dem.width.saturating_sub(1));
    let y1 = (y0 + 1).min(dem.height.saturating_sub(1));
    let tx = sample_x.fract() as f32;
    let ty = sample_y.fract() as f32;

    let top_left = dem.samples[(y0 * dem.width + x0) as usize];
    let top_right = dem.samples[(y0 * dem.width + x1) as usize];
    let bottom_left = dem.samples[(y1 * dem.width + x0) as usize];
    let bottom_right = dem.samples[(y1 * dem.width + x1) as usize];

    let top = top_left * (1.0 - tx) + top_right * tx;
    let bottom = bottom_left * (1.0 - tx) + bottom_right * tx;
    top * (1.0 - ty) + bottom * ty
}

fn encode_height_tiff(
    width: u32,
    height: u32,
    heights: &[f32],
) -> Result<Vec<u8>, StreamingProviderError> {
    let mut cursor = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut cursor).map_err(|error| {
        StreamingProviderError::Permanent(format!("failed to create TIFF encoder: {error}"))
    })?;
    encoder
        .write_image::<colortype::Gray32Float>(width, height, heights)
        .map_err(|error| {
            StreamingProviderError::Permanent(format!("failed to encode TIFF tile: {error}"))
        })?;
    Ok(cursor.into_inner())
}

fn map_ureq_error(error: ureq::Error) -> StreamingProviderError {
    match error {
        ureq::Error::Status(code, response) => {
            let status = format!(
                "OpenTopography returned HTTP {code} for {}",
                redact_api_key_in_url(response.get_url())
            );
            if code == 429 || code >= 500 {
                StreamingProviderError::Transient(status)
            } else {
                StreamingProviderError::Permanent(status)
            }
        }
        ureq::Error::Transport(error) => StreamingProviderError::Transient(format!(
            "OpenTopography transport failed: {}",
            error.message().unwrap_or("transport error without message")
        )),
    }
}

fn redact_api_key_in_url(url: &str) -> String {
    redact_query_parameter(url, "API_Key")
}

fn redact_query_parameter(url: &str, parameter: &str) -> String {
    let needle = format!("{parameter}=");
    let Some(start) = url.find(&needle) else {
        return url.to_string();
    };
    let value_start = start + needle.len();
    let value_end = url[value_start..]
        .find('&')
        .map(|offset| value_start + offset)
        .unwrap_or(url.len());
    let mut redacted = url.to_string();
    redacted.replace_range(value_start..value_end, "REDACTED");
    redacted
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
        math::{Coordinate, TerrainShape, TileCoordinate, ViewCoordinate},
        streaming::{StreamingRequestPriority, cache_writer::write_materialized_tile},
        terrain_data::{AttachmentConfig, AttachmentLabel},
    };
    use bevy::math::IVec2;
    use std::{
        fs,
        io::Cursor,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use tiff::decoder::{Decoder, DecodingResult};

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
            terrain_lod_count: 5,
            priority: StreamingRequestPriority::Background,
        }
    }

    #[test]
    fn provider_plans_official_opentopography_requests() {
        let provider = OpenTopographyHeightProvider::new(OpenTopographyHeightConfig {
            api_key: Some("test-key".to_string()),
            ..Default::default()
        });
        let planned = provider
            .plan_global_dem(&height_request(TileCoordinate::new(
                0,
                6,
                IVec2::new(20, 18),
            )))
            .expect("mid-latitude tile should be plannable");

        let url = planned.url();
        assert!(url.starts_with(DEFAULT_OPENTOPOGRAPHY_ENDPOINT));
        assert!(url.contains("demtype=AW3D30_E"));
        assert!(url.contains("outputFormat=GTiff"));
        assert!(url.contains("API_Key=test-key"));
    }

    #[test]
    fn provider_requires_api_key() {
        let provider = OpenTopographyHeightProvider::new(OpenTopographyHeightConfig {
            api_key: None,
            ..Default::default()
        });

        assert!(matches!(
            provider.availability(&height_request(TileCoordinate::new(
                0,
                6,
                IVec2::new(20, 18)
            ))),
            StreamingSourceAvailability::Unavailable { .. }
        ));
    }

    #[test]
    fn provider_rejects_imagery_requests() {
        let provider = OpenTopographyHeightProvider::new(OpenTopographyHeightConfig {
            api_key: Some("test-key".to_string()),
            ..Default::default()
        });
        let mut request = height_request(TileCoordinate::new(0, 2, IVec2::new(1, 1)));
        request.attachment_label = AttachmentLabel::Custom("albedo".to_string());

        assert!(matches!(
            provider.availability(&request),
            StreamingSourceAvailability::Unavailable { .. }
        ));
    }

    #[test]
    fn provider_rejects_requests_that_exceed_aw3d30_area_limit() {
        let provider = OpenTopographyHeightProvider::new(OpenTopographyHeightConfig {
            api_key: Some("test-key".to_string()),
            ..Default::default()
        });
        let request = height_request(TileCoordinate::new(0, 3, IVec2::new(5, 0)));

        let error = provider
            .plan_global_dem(&request)
            .expect_err("coarse AW3D30 tiles should exceed the documented area limit");

        let message = error.to_string();
        assert!(message.contains("450000 km^2"));
        assert!(message.contains("covers about"));
    }

    #[test]
    fn api_key_is_redacted_from_error_urls() {
        let redacted = redact_api_key_in_url(
            "https://portal.opentopography.org/API/globaldem?demtype=AW3D30_E&API_Key=secret-value&outputFormat=GTiff",
        );

        assert!(redacted.contains("API_Key=REDACTED"));
        assert!(!redacted.contains("secret-value"));
    }

    #[test]
    fn html_response_is_reported_as_non_tiff() {
        let error = decode_dem_tiff(
            b"<!DOCTYPE html><html><body>rate limited</body></html>",
            "text/html",
        )
        .expect_err("html error pages should not reach the TIFF decoder");

        let message = error.to_string();
        assert!(message.contains("non-TIFF response"));
        assert!(message.contains("text/html"));
        assert!(message.contains("rate limited"));
    }

    #[test]
    fn decode_uses_configured_tiff_limits() {
        let heights = vec![1234.0_f32; 1024 * 1024];
        let bytes = encode_height_tiff(1024, 1024, &heights).expect("test TIFF should encode");
        let mut tight_limits = Limits::default();
        tight_limits.decoding_buffer_size = 1024 * 1024;
        tight_limits.intermediate_buffer_size = 1024 * 1024;
        tight_limits.ifd_value_size = OPENTOPOGRAPHY_MAX_IFD_VALUE_BYTES;

        let error = decode_dem_tiff_with_limits(&bytes, "image/tiff", tight_limits)
            .expect_err("artificially small limits should reject the TIFF");
        assert!(error.to_string().contains("decoder limits exceeded"));

        let decoded = decode_dem_tiff(&bytes, "image/tiff")
            .expect("provider defaults should decode modest TIFF payloads");
        assert_eq!(decoded.width, 1024);
        assert_eq!(decoded.height, 1024);
        assert_eq!(decoded.samples.len(), heights.len());
    }

    fn unique_temp_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("terrain_opentopography_live_{unique}"))
    }

    #[test]
    #[ignore = "requires OPENTOPOGRAPHY_API_KEY and live network access"]
    fn live_provider_materializes_and_caches_a_height_tile() {
        if std::env::var("OPENTOPOGRAPHY_API_KEY").is_err()
            && std::env::var("OPEN_TOPOGRAPHY_API_KEY").is_err()
        {
            panic!("OPENTOPOGRAPHY_API_KEY is required for the live smoke test");
        }

        let provider = OpenTopographyHeightProvider::default();
        let sample_coordinate = Coordinate::from_lat_lon_degrees(37.705, -122.495);
        let view_coordinate = ViewCoordinate::new(sample_coordinate, 6);
        let request = height_request(TileCoordinate::new(
            sample_coordinate.face,
            6,
            view_coordinate.xy,
        ));
        let tile = provider
            .materialize_tile(&request)
            .expect("live provider should return a TIFF height tile");

        let asset_root = unique_temp_dir();
        fs::create_dir_all(&asset_root).unwrap();
        let cache_root = PathBuf::from("streaming_cache");
        let written = write_materialized_tile(&asset_root, &cache_root, &tile)
            .expect("cache writer should persist the live tile");

        let mut decoder = Decoder::new(Cursor::new(fs::read(asset_root.join(written)).unwrap()))
            .expect("cached tile should remain TIFF-decodable");
        let (width, height) = decoder.dimensions().unwrap();
        assert_eq!(width, request.attachment_config.texture_size);
        assert_eq!(height, request.attachment_config.texture_size);
        match decoder.read_image().unwrap() {
            DecodingResult::F32(values) => assert!(!values.is_empty()),
            other => panic!("expected F32 TIFF payload, got {other:?}"),
        }

        fs::remove_dir_all(asset_root).unwrap();
    }
}
