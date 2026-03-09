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
    decoder::{Decoder, DecodingResult},
    encoder::{TiffEncoder, colortype},
};

const DEFAULT_OPENTOPOGRAPHY_ENDPOINT: &str = "https://portal.opentopography.org/API/globaldem";
const DEFAULT_OPENTOPOGRAPHY_DEM_TYPE: &str = "AW3D30_E";
const DEFAULT_OPENTOPOGRAPHY_OUTPUT_FORMAT: &str = "GTiff";
const PLAUSIBLE_EARTH_MIN_HEIGHT_M: f32 = -20_000.0;
const PLAUSIBLE_EARTH_MAX_HEIGHT_M: f32 = 20_000.0;

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
        let mut body = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|error| {
                StreamingProviderError::Transient(format!("opentopography read failed: {error}"))
            })?;

        let source_dem = decode_dem_tiff(&body)?;
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

struct DecodedDem {
    width: u32,
    height: u32,
    samples: Vec<f32>,
}

fn decode_dem_tiff(bytes: &[u8]) -> Result<DecodedDem, StreamingProviderError> {
    let mut decoder = Decoder::new(Cursor::new(bytes)).map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to construct OpenTopography TIFF decoder: {error}"
        ))
    })?;
    let (width, height) = decoder.dimensions().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to read OpenTopography TIFF dimensions: {error}"
        ))
    })?;

    let samples = match decoder.read_image().map_err(|error| {
        StreamingProviderError::Permanent(format!(
            "failed to decode OpenTopography TIFF body: {error}"
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
                response.get_url()
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
        streaming::cache_writer::write_materialized_tile,
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
        }
    }

    #[test]
    fn provider_plans_official_opentopography_requests() {
        let provider = OpenTopographyHeightProvider::new(OpenTopographyHeightConfig {
            api_key: Some("test-key".to_string()),
            ..Default::default()
        });
        let planned = provider
            .plan_global_dem(&height_request(TileCoordinate::new(0, 2, IVec2::new(1, 1))))
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
            provider.availability(&height_request(TileCoordinate::new(0, 2, IVec2::new(1, 1)))),
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
