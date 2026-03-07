use bevy::math::DVec3;

pub const WGS84_SEMIMAJOR_AXIS_M: f64 = 6_378_137.0;
pub const WGS84_SEMIMINOR_AXIS_M: f64 = 6_356_752.314_245_18;
pub const WGS84_FIRST_ECCENTRICITY_SQ: f64 = 0.006_694_379_990_141_33;
pub const WGS84_SECOND_ECCENTRICITY_SQ: f64 =
    WGS84_FIRST_ECCENTRICITY_SQ / (1.0 - WGS84_FIRST_ECCENTRICITY_SQ);

#[inline(always)]
fn wgs84_renderer_scale() -> DVec3 {
    DVec3::new(
        WGS84_SEMIMAJOR_AXIS_M,
        WGS84_SEMIMINOR_AXIS_M,
        WGS84_SEMIMAJOR_AXIS_M,
    )
}

#[inline(always)]
fn renderer_local_from_ecef(ecef: DVec3) -> DVec3 {
    DVec3::new(-ecef.x, ecef.z, ecef.y)
}

#[inline(always)]
fn ecef_from_renderer_local(local: DVec3) -> DVec3 {
    DVec3::new(-local.x, local.z, local.y)
}

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
    let surface_ecef = lla_hae_to_ecef(LlaHae {
        lat_deg,
        lon_deg,
        hae_m: 0.0,
    });
    let surface_local = renderer_local_from_ecef(surface_ecef);
    (surface_local / wgs84_renderer_scale()).normalize()
}

#[inline(always)]
/// Converts a renderer ellipsoid-chart unit position into geodetic latitude/longitude.
/// The input is expected to describe a point on the unit sphere chart used by the renderer.
pub fn lat_lon_degrees_from_unit(unit_position: DVec3) -> (f64, f64) {
    let surface_local = wgs84_renderer_scale() * unit_position.normalize();
    let surface_ecef = ecef_from_renderer_local(surface_local);
    let lla = ecef_to_lla_hae(surface_ecef);
    (lla.lat_deg, lla.lon_deg)
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
    use bevy::math::DVec3;
    use crate::math::TerrainShape;
    use small_world::wgs84::{AltType, Lla as SwLla, Ned as SwNed};

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

    fn renderer_local_from_small_world_lla(lat_deg: f64, lon_deg: f64, hae_m: f64) -> DVec3 {
        let ecef = SwLla::new(lat_deg, lon_deg, hae_m, AltType::Wgs84).to_ecef();
        DVec3::new(-ecef.x(), ecef.z(), ecef.y())
    }

    #[test]
    fn small_world_lla_to_ecef_matches_renderer_geodesy() {
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
            LlaHae {
                lat_deg: 0.0,
                lon_deg: 0.0,
                hae_m: 0.0,
            },
        ];

        for lla in cases {
            let renderer = lla_hae_to_ecef(lla);
            let small_world = SwLla::new(lla.lat_deg, lla.lon_deg, lla.hae_m, AltType::Wgs84)
                .to_ecef();

            assert!((renderer.x - small_world.x()).abs() < 1e-6);
            assert!((renderer.y - small_world.y()).abs() < 1e-6);
            assert!((renderer.z - small_world.z()).abs() < 1e-6);
        }
    }

    #[test]
    fn small_world_ecef_to_lla_matches_renderer_geodesy() {
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
            let ecef = SwLla::new(lla.lat_deg, lla.lon_deg, lla.hae_m, AltType::Wgs84).to_ecef();
            let renderer = ecef_to_lla_hae(DVec3::new(ecef.x(), ecef.y(), ecef.z()));
            let small_world = SwLla::from_ecef(ecef);

            assert!((renderer.lat_deg - small_world.lat_deg()).abs() < 1e-9);
            assert!((normalize_lon(renderer.lon_deg) - normalize_lon(small_world.lon_deg())).abs() < 1e-9);
            assert!((renderer.hae_m - small_world.alt_m()).abs() < 1e-4);
        }
    }

    #[test]
    fn small_world_ned_to_ecef_matches_renderer_geodesy() {
        let cases = [
            (
                LlaHae {
                    lat_deg: 46.55,
                    lon_deg: 10.6,
                    hae_m: 2920.0,
                },
                Ned {
                    n_m: 152.0,
                    e_m: -88.5,
                    d_m: 17.25,
                },
            ),
            (
                LlaHae {
                    lat_deg: 25.7617,
                    lon_deg: -80.1918,
                    hae_m: 110.0,
                },
                Ned {
                    n_m: -320.0,
                    e_m: 45.0,
                    d_m: -12.5,
                },
            ),
        ];

        for (origin, ned) in cases {
            let renderer = ned_to_ecef(ned, origin);
            let sw_origin =
                SwLla::new(origin.lat_deg, origin.lon_deg, origin.hae_m, AltType::Wgs84);
            let small_world = SwNed::new(ned.n_m, ned.e_m, ned.d_m, sw_origin).to_ecef();

            assert!((renderer.x - small_world.x()).abs() < 1e-6);
            assert!((renderer.y - small_world.y()).abs() < 1e-6);
            assert!((renderer.z - small_world.z()).abs() < 1e-6);

            let renderer_ned = ecef_to_ned(
                DVec3::new(small_world.x(), small_world.y(), small_world.z()),
                origin,
            );
            let small_world_ned = SwNed::from_ecef(small_world, sw_origin);

            assert!((renderer_ned.n_m - small_world_ned.n()).abs() < 1e-6);
            assert!((renderer_ned.e_m - small_world_ned.e()).abs() < 1e-6);
            assert!((renderer_ned.d_m - small_world_ned.d()).abs() < 1e-6);
        }
    }

    #[test]
    fn unit_from_lat_lon_matches_small_world_wgs84_surface() {
        let cases = [
            (0.0, 0.0),
            (46.55, 10.6),
            (25.7617, -80.1918),
            (-33.9, 151.2),
            (89.5, 179.0),
        ];

        for (lat_deg, lon_deg) in cases {
            let renderer_unit = unit_from_lat_lon_degrees(lat_deg, lon_deg);
            let renderer_surface = TerrainShape::WGS84.scale() * renderer_unit;
            let small_world_surface = renderer_local_from_small_world_lla(lat_deg, lon_deg, 0.0);

            assert!((renderer_surface - small_world_surface).length() < 1e-3);

            let (round_lat, round_lon) = lat_lon_degrees_from_unit(renderer_unit);
            assert!((round_lat - lat_deg).abs() < 1e-9);
            assert!((normalize_lon(round_lon) - normalize_lon(lon_deg)).abs() < 1e-9);
        }
    }

    #[test]
    fn renderer_wgs84_local_mapping_matches_small_world() {
        let cases = [
            (0.0, 0.0, 0.0),
            (46.55, 10.6, 2920.0),
            (25.7617, -80.1918, 110.0),
            (-33.9, 151.2, 30.0),
            (89.5, 179.0, 0.0),
        ];

        for (lat_deg, lon_deg, hae_m) in cases {
            let renderer_local = crate::math::Coordinate::from_lat_lon_degrees(lat_deg, lon_deg)
                .local_position(TerrainShape::WGS84, hae_m as f32);
            let small_world_local = renderer_local_from_small_world_lla(lat_deg, lon_deg, hae_m);
            let error_m = (renderer_local - small_world_local).length();

            assert!(
                error_m < 1e-3,
                "renderer local mapping delta @ lat={lat_deg:.5}, lon={lon_deg:.5}, hae={hae_m:.2}: {error_m:.6} m"
            );
        }
    }

    #[test]
    fn small_world_ned_orbit_path_maps_to_renderer_local_positions() {
        let origins = [
            LlaHae {
                lat_deg: 46.55,
                lon_deg: 10.6,
                hae_m: 2920.0,
            },
            LlaHae {
                lat_deg: 24.70,
                lon_deg: -81.30,
                hae_m: 0.0,
            },
            LlaHae {
                lat_deg: -33.9,
                lon_deg: 151.2,
                hae_m: 30.0,
            },
        ];

        for origin in origins {
            let sw_origin =
                SwLla::new(origin.lat_deg, origin.lon_deg, origin.hae_m, AltType::Wgs84);

            for &radius_m in &[100.0_f64, 1000.0_f64] {
                for sample_index in 0..64 {
                    let theta = std::f64::consts::TAU * sample_index as f64 / 64.0;
                    let ned = Ned {
                        n_m: radius_m * theta.cos(),
                        e_m: radius_m * theta.sin(),
                        d_m: -120.0,
                    };

                    let sw_ecef = SwNed::new(ned.n_m, ned.e_m, ned.d_m, sw_origin).to_ecef();
                    let sw_lla = SwLla::from_ecef(sw_ecef);

                    let renderer_from_small_world_path =
                        crate::math::Coordinate::from_lat_lon_degrees(
                            sw_lla.lat_deg(),
                            sw_lla.lon_deg(),
                        )
                        .local_position(TerrainShape::WGS84, sw_lla.alt_m() as f32);

                    let renderer_from_small_world_ecef =
                        DVec3::new(-sw_ecef.x(), sw_ecef.z(), sw_ecef.y());

                    let direct_renderer_ecef = ned_to_ecef(ned, origin);
                    let direct_renderer_local = DVec3::new(
                        -direct_renderer_ecef.x,
                        direct_renderer_ecef.z,
                        direct_renderer_ecef.y,
                    );

                    let path_error_m = (
                        renderer_from_small_world_path - renderer_from_small_world_ecef
                    )
                    .length();
                    let ned_error_m = (direct_renderer_local - renderer_from_small_world_ecef)
                        .length();

                    assert!(
                        path_error_m < 1e-3,
                        "small_world orbit path mapping error @ origin=({:.5},{:.5},{:.2}) radius={:.1} sample={} => {:.6} m",
                        origin.lat_deg,
                        origin.lon_deg,
                        origin.hae_m,
                        radius_m,
                        sample_index,
                        path_error_m
                    );
                    assert!(
                        ned_error_m < 1e-6,
                        "renderer ned/ecef mismatch @ origin=({:.5},{:.5},{:.2}) radius={:.1} sample={} => {:.9} m",
                        origin.lat_deg,
                        origin.lon_deg,
                        origin.hae_m,
                        radius_m,
                        sample_index,
                        ned_error_m
                    );
                }
            }
        }
    }
}
