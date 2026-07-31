//! Reference-frame facade — the geodesy authority.
//!
//! [`small_world`] is the **single source of truth** for all WGS84 reference-frame math (LLA
//! HAE/MSL ↔ ECEF ↔ ENU/NED, geoid undulation). This module is heimdall's one seam over it:
//! renderer code imports frames from here, never from `small_world` directly, so the engine-axis
//! convention and the floating-origin transform (added at M0 with `big_space`) live in exactly
//! one place.
//!
//! Vertical-datum contract: `docs/reference_frames.md` (normative). The short version: the render
//! path speaks **HAE only**; MSL and AGL convert to HAE at their ingest boundaries.

pub use small_world::altitude::{AltitudeConverter, GeoPoint, VerticalFrame};
pub use small_world::egm96::EGM96;
pub use small_world::wgs84::{AltType, Ecef, Enu, Lla, Ned};

/// Builds a geodetic point with an explicit **ellipsoidal (HAE)** altitude — the only vertical
/// datum the render path accepts. MSL/AGL feeds must convert first (see
/// `docs/reference_frames.md` §1).
pub fn lla_hae(lat_deg: f64, lon_deg: f64, hae_m: f64) -> Lla {
    Lla::new(lat_deg, lon_deg, hae_m, AltType::Wgs84)
}

/// LLA (HAE) → geodetic ECEF metres.
pub fn ecef_from_lla(lla: Lla) -> Ecef {
    lla.to_ecef()
}

/// Geodetic ECEF metres → LLA (HAE).
pub fn lla_from_ecef(ecef: Ecef) -> Lla {
    Lla::from_ecef(ecef)
}

// M0 adds here, and only here:
//   - the fixed rotation between geodetic ECEF axes and the engine's coordinate handedness;
//   - `ecef_to_render(ecef) -> GridCell + Vec3` via the big_space floating origin;
//   - the AGL query hook against resident terrain tiles (M2+).

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lla_ecef_round_trip_through_the_facade() {
        let cases = [
            (0.0, 0.0, 0.0),
            (51.4779, -0.0015, 46.0),   // Greenwich Observatory area
            (27.9881, 86.925, 8823.0),  // Everest summit (HAE ≈ 8848.86 MSL − 28.7 geoid + snow)
            (-33.8568, 151.2153, 25.0), // Sydney
            (89.5, 179.0, 0.0),         // near-pole, near-antimeridian
        ];

        for (lat_deg, lon_deg, hae_m) in cases {
            let ecef = ecef_from_lla(lla_hae(lat_deg, lon_deg, hae_m));
            let round = lla_from_ecef(ecef);

            assert!((round.lat_deg() - lat_deg).abs() < 1e-9, "lat @ {lat_deg}");
            assert!(
                (normalize_lon(round.lon_deg()) - normalize_lon(lon_deg)).abs() < 1e-9,
                "lon @ {lon_deg}"
            );
            assert!((round.alt_m() - hae_m).abs() < 1e-4, "alt @ {hae_m}");
        }
    }

    #[test]
    fn ecef_magnitude_matches_wgs84_axes() {
        // Equator on the prime meridian sits at exactly one semi-major axis from the origin.
        let equator = ecef_from_lla(lla_hae(0.0, 0.0, 0.0));
        assert!((equator.x() - 6_378_137.0).abs() < 1e-6);
        assert!(equator.y().abs() < 1e-6);
        assert!(equator.z().abs() < 1e-6);

        // The pole sits at one semi-minor axis.
        let pole = ecef_from_lla(lla_hae(90.0, 0.0, 0.0));
        assert!((pole.z() - 6_356_752.314_245_18).abs() < 1e-4);
    }
}
