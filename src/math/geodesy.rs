use bevy::math::DVec3;

pub const WGS84_SEMIMAJOR_AXIS_M: f64 = 6_378_137.0;
pub const WGS84_SEMIMINOR_AXIS_M: f64 = 6_356_752.314_245_18;
pub const WGS84_FIRST_ECCENTRICITY_SQ: f64 = 0.006_694_379_990_141_33;
pub const WGS84_SECOND_ECCENTRICITY_SQ: f64 =
    WGS84_FIRST_ECCENTRICITY_SQ / (1.0 - WGS84_FIRST_ECCENTRICITY_SQ);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LlaHae {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub hae_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ned {
    pub n_m: f64,
    pub e_m: f64,
    pub d_m: f64,
}

#[derive(Clone, Copy, Debug)]
struct GeoConversionParams {
    rot: [[f64; 3]; 3],
    x0: f64,
    y0: f64,
    z0: f64,
}

impl GeoConversionParams {
    #[inline(always)]
    fn from_origin(origin: LlaHae) -> Self {
        let lat0 = origin.lat_deg.to_radians();
        let lon0 = origin.lon_deg.to_radians();
        let alt0 = origin.hae_m;
        let nu0 = WGS84_SEMIMAJOR_AXIS_M
            / (1.0 - WGS84_FIRST_ECCENTRICITY_SQ * lat0.sin() * lat0.sin()).sqrt();
        let s_lat0 = lat0.sin();
        let c_lat0 = lat0.cos();
        let s_lon0 = lon0.sin();
        let c_lon0 = lon0.cos();

        let rot = [
            [-s_lat0 * c_lon0, -s_lat0 * s_lon0, c_lat0],
            [-s_lon0, c_lon0, 0.0],
            [-c_lat0 * c_lon0, -c_lat0 * s_lon0, -s_lat0],
        ];

        let x0 = (nu0 + alt0) * c_lat0 * c_lon0;
        let y0 = (nu0 + alt0) * c_lat0 * s_lon0;
        let z0 = (nu0 * (1.0 - WGS84_FIRST_ECCENTRICITY_SQ) + alt0) * s_lat0;

        Self { rot, x0, y0, z0 }
    }
}

#[inline(always)]
pub fn unit_from_lat_lon_degrees(lat_deg: f64, lon_deg: f64) -> DVec3 {
    let lat = lat_deg.to_radians();
    let lon = lon_deg.to_radians();
    DVec3::new(-lat.cos() * lon.cos(), lat.sin(), lat.cos() * lon.sin())
}

#[inline(always)]
/// Converts a renderer unit-sphere direction into geodetic latitude/longitude.
/// The input is expected to be normalized.
pub fn lat_lon_degrees_from_unit(unit_position: DVec3) -> (f64, f64) {
    let lon_deg = unit_position.z.atan2(-unit_position.x).to_degrees();
    let lat_deg = unit_position.y.asin().to_degrees();
    (lat_deg, lon_deg)
}

#[inline(always)]
pub fn lla_hae_to_ecef(lla: LlaHae) -> DVec3 {
    let lat = lla.lat_deg.to_radians();
    let lon = lla.lon_deg.to_radians();
    let s_lat = lat.sin();
    let c_lat = lat.cos();
    let s_lon = lon.sin();
    let c_lon = lon.cos();
    let nu = WGS84_SEMIMAJOR_AXIS_M / (1.0 - WGS84_FIRST_ECCENTRICITY_SQ * s_lat * s_lat).sqrt();

    DVec3::new(
        (nu + lla.hae_m) * c_lat * c_lon,
        (nu + lla.hae_m) * c_lat * s_lon,
        (nu * (1.0 - WGS84_FIRST_ECCENTRICITY_SQ) + lla.hae_m) * s_lat,
    )
}

#[inline(always)]
pub fn ecef_to_lla_hae(ecef: DVec3) -> LlaHae {
    let p = (ecef.x * ecef.x + ecef.y * ecef.y).sqrt();
    let q = (ecef.z * WGS84_SEMIMAJOR_AXIS_M).atan2(p * WGS84_SEMIMINOR_AXIS_M);
    let lat = (ecef.z + WGS84_SECOND_ECCENTRICITY_SQ * WGS84_SEMIMINOR_AXIS_M * q.sin().powi(3))
        .atan2(p - WGS84_FIRST_ECCENTRICITY_SQ * WGS84_SEMIMAJOR_AXIS_M * q.cos().powi(3));
    let lon = ecef.y.atan2(ecef.x);
    let nu =
        WGS84_SEMIMAJOR_AXIS_M / (1.0 - WGS84_FIRST_ECCENTRICITY_SQ * lat.sin() * lat.sin()).sqrt();
    let hae = p / lat.cos() - nu;

    LlaHae {
        lat_deg: lat.to_degrees(),
        lon_deg: lon.to_degrees(),
        hae_m: hae,
    }
}

#[inline(always)]
pub fn ned_to_ecef(ned: Ned, origin: LlaHae) -> DVec3 {
    let cp = GeoConversionParams::from_origin(origin);

    let dx = cp.rot[0][0] * ned.n_m + cp.rot[1][0] * ned.e_m + cp.rot[2][0] * ned.d_m;
    let dy = cp.rot[0][1] * ned.n_m + cp.rot[1][1] * ned.e_m + cp.rot[2][1] * ned.d_m;
    let dz = cp.rot[0][2] * ned.n_m + cp.rot[1][2] * ned.e_m + cp.rot[2][2] * ned.d_m;

    DVec3::new(dx + cp.x0, dy + cp.y0, dz + cp.z0)
}

#[inline(always)]
pub fn ecef_to_ned(ecef: DVec3, origin: LlaHae) -> Ned {
    let cp = GeoConversionParams::from_origin(origin);

    let dx = ecef.x - cp.x0;
    let dy = ecef.y - cp.y0;
    let dz = ecef.z - cp.z0;

    Ned {
        n_m: cp.rot[0][0] * dx + cp.rot[0][1] * dy + cp.rot[0][2] * dz,
        e_m: cp.rot[1][0] * dx + cp.rot[1][1] * dy + cp.rot[1][2] * dz,
        d_m: cp.rot[2][0] * dx + cp.rot[2][1] * dy + cp.rot[2][2] * dz,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LlaHae, Ned, ecef_to_lla_hae, ecef_to_ned, lat_lon_degrees_from_unit, lla_hae_to_ecef,
        ned_to_ecef, unit_from_lat_lon_degrees,
    };

    fn normalize_lon(lon_deg: f64) -> f64 {
        let mut lon = lon_deg % 360.0;
        if lon > 180.0 {
            lon -= 360.0;
        } else if lon < -180.0 {
            lon += 360.0;
        }
        lon
    }

    #[test]
    fn lat_lon_unit_roundtrip_is_stable() {
        let cases = [
            (0.0, 0.0),
            (37.7749, -122.4194),
            (46.55, 10.6),
            (-33.8568, 151.2153),
            (89.999, 179.9),
            (-89.999, -179.9),
        ];

        for (lat_deg, lon_deg) in cases {
            let unit = unit_from_lat_lon_degrees(lat_deg, lon_deg);
            let (round_lat, round_lon) = lat_lon_degrees_from_unit(unit);

            assert!((unit.length() - 1.0).abs() < 1e-12);
            assert!((round_lat - lat_deg).abs() < 1e-9);
            assert!((normalize_lon(round_lon) - normalize_lon(lon_deg)).abs() < 1e-9);
        }
    }

    #[test]
    fn lla_ecef_roundtrip_is_stable() {
        let cases = [
            LlaHae {
                lat_deg: 46.55,
                lon_deg: 10.6,
                hae_m: 2920.0,
            },
            LlaHae {
                lat_deg: 25.7617,
                lon_deg: -80.1918,
                hae_m: 110.0,
            },
            LlaHae {
                lat_deg: -33.9,
                lon_deg: 151.2,
                hae_m: 30.0,
            },
        ];

        for lla in cases {
            let ecef = lla_hae_to_ecef(lla);
            let round = ecef_to_lla_hae(ecef);

            assert!((round.lat_deg - lla.lat_deg).abs() < 1e-9);
            assert!((normalize_lon(round.lon_deg) - normalize_lon(lla.lon_deg)).abs() < 1e-9);
            assert!((round.hae_m - lla.hae_m).abs() < 1e-4);
        }
    }

    #[test]
    fn ned_ecef_roundtrip_is_stable() {
        let origin = LlaHae {
            lat_deg: 46.55,
            lon_deg: 10.6,
            hae_m: 2920.0,
        };
        let ned = Ned {
            n_m: 152.0,
            e_m: -88.5,
            d_m: 17.25,
        };

        let ecef = ned_to_ecef(ned, origin);
        let round = ecef_to_ned(ecef, origin);

        assert!((round.n_m - ned.n_m).abs() < 1e-6);
        assert!((round.e_m - ned.e_m).abs() < 1e-6);
        assert!((round.d_m - ned.d_m).abs() < 1e-6);
    }
}
