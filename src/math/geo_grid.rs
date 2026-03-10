use super::{
    Enu, LlaHae, Ned, ecef_to_renderer_local, enu_to_renderer_local, lla_to_renderer_local,
    ned_to_renderer_local,
};
use bevy::math::{DVec3, Vec3};
use big_space::prelude::{CellCoord, Grid};

/// Extension trait for translating geo-referenced positions directly into a `Grid`.
pub trait GridGeoExt {
    /// Convert an absolute geodetic position into a grid cell and local translation.
    fn translation_from_lla(&self, lla: LlaHae) -> (CellCoord, Vec3);

    /// Convert an absolute ECEF position into a grid cell and local translation.
    fn translation_from_ecef(&self, ecef: DVec3) -> (CellCoord, Vec3);

    /// Convert a local NED offset about `origin` into a grid cell and local translation.
    fn translation_from_ned(&self, ned: Ned, origin: LlaHae) -> (CellCoord, Vec3);

    /// Convert a local ENU offset about `origin` into a grid cell and local translation.
    fn translation_from_enu(&self, enu: Enu, origin: LlaHae) -> (CellCoord, Vec3);
}

impl GridGeoExt for Grid {
    fn translation_from_lla(&self, lla: LlaHae) -> (CellCoord, Vec3) {
        self.translation_to_grid(lla_to_renderer_local(lla))
    }

    fn translation_from_ecef(&self, ecef: DVec3) -> (CellCoord, Vec3) {
        self.translation_to_grid(ecef_to_renderer_local(ecef))
    }

    fn translation_from_ned(&self, ned: Ned, origin: LlaHae) -> (CellCoord, Vec3) {
        self.translation_to_grid(ned_to_renderer_local(ned, origin))
    }

    fn translation_from_enu(&self, enu: Enu, origin: LlaHae) -> (CellCoord, Vec3) {
        self.translation_to_grid(enu_to_renderer_local(enu, origin))
    }
}

#[cfg(test)]
mod tests {
    use super::GridGeoExt;
    use crate::math::geodesy::{ecef_to_lla_hae, ned_to_ecef};
    use crate::math::{Enu, LlaHae, Ned, ecef_to_renderer_local};
    use bevy::math::Vec3;
    use big_space::prelude::Grid;

    fn assert_translation_close(actual: Vec3, expected: Vec3) {
        assert!((actual - expected).length() < 1e-4);
    }

    #[test]
    fn grid_geo_ext_methods_match_equivalent_points() {
        let grid = Grid::new(2_000.0, 100.0);
        let origin = LlaHae {
            lat_deg: 37.77,
            lon_deg: -122.42,
            hae_m: 16.0,
        };
        let ned = Ned {
            n_m: 300.0,
            e_m: -150.0,
            d_m: -100.0,
        };
        let enu = Enu {
            e_m: ned.e_m,
            n_m: ned.n_m,
            u_m: -ned.d_m,
        };
        let ecef = ned_to_ecef(ned, origin);
        let lla = ecef_to_lla_hae(ecef);

        let expected = grid.translation_to_grid(ecef_to_renderer_local(ecef));
        let from_lla = grid.translation_from_lla(lla);
        let from_ecef = grid.translation_from_ecef(ecef);
        let from_ned = grid.translation_from_ned(ned, origin);
        let from_enu = grid.translation_from_enu(enu, origin);

        assert_eq!(from_lla.0, expected.0);
        assert_eq!(from_ecef.0, expected.0);
        assert_eq!(from_ned.0, expected.0);
        assert_eq!(from_enu.0, expected.0);

        assert_translation_close(from_lla.1, expected.1);
        assert_translation_close(from_ecef.1, expected.1);
        assert_translation_close(from_ned.1, expected.1);
        assert_translation_close(from_enu.1, expected.1);
    }

    #[test]
    fn prelude_geo_api_path_compiles() {
        use crate::prelude::{CellCoord, Enu, Grid, GridGeoExt, LlaHae, Ned};

        let grid = Grid::new(2_000.0, 100.0);
        let origin = LlaHae {
            lat_deg: 37.77,
            lon_deg: -122.42,
            hae_m: 16.0,
        };
        let ned = Ned {
            n_m: 300.0,
            e_m: -150.0,
            d_m: -100.0,
        };
        let enu = Enu {
            e_m: ned.e_m,
            n_m: ned.n_m,
            u_m: -ned.d_m,
        };

        let (cell_from_ned, _) = grid.translation_from_ned(ned, origin);
        let (cell_from_enu, _) = grid.translation_from_enu(enu, origin);
        let _: CellCoord = cell_from_ned;

        assert_eq!(cell_from_ned, cell_from_enu);
    }
}
