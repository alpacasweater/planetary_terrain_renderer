// Demo scene setup, overlay selection, and camera/drone motion helpers.

use super::*;

pub fn overlay_config_map() -> HashMap<&'static str, OverlayPreset> {
    HashMap::from([
        (
            "swiss",
            OverlayPreset {
                config_path: "terrains/swiss_highres/config.tc.ron",
                truth_source_raster_path: Some("source_data/swiss.tif"),
                label: "Swiss Alps",
                focus_lat_deg: 46.8,
                focus_lon_deg: 8.2,
            },
        ),
        (
            "saxony",
            OverlayPreset {
                config_path: "terrains/saxony_partial/config.tc.ron",
                truth_source_raster_path: None,
                label: "Saxony",
                focus_lat_deg: 50.9,
                focus_lon_deg: 13.5,
            },
        ),
        (
            "los",
            OverlayPreset {
                config_path: "terrains/los_highres/config.tc.ron",
                truth_source_raster_path: None,
                label: "Los Angeles",
                focus_lat_deg: 34.05,
                focus_lon_deg: -118.25,
            },
        ),
        (
            "srtm_n27e086",
            OverlayPreset {
                config_path: "terrains/srtm_n27e086/config.tc.ron",
                truth_source_raster_path: None,
                label: "Himalaya",
                focus_lat_deg: 27.9,
                focus_lon_deg: 86.9,
            },
        ),
        (
            "srtm_n35e139",
            OverlayPreset {
                config_path: "terrains/srtm_n35e139/config.tc.ron",
                truth_source_raster_path: None,
                label: "Tokyo",
                focus_lat_deg: 35.68,
                focus_lon_deg: 139.76,
            },
        ),
        (
            "srtm_n37e127",
            OverlayPreset {
                config_path: "terrains/srtm_n37e127/config.tc.ron",
                truth_source_raster_path: None,
                label: "Korea",
                focus_lat_deg: 37.57,
                focus_lon_deg: 126.98,
            },
        ),
        (
            "srtm_n39w077",
            OverlayPreset {
                config_path: "terrains/srtm_n39w077/config.tc.ron",
                truth_source_raster_path: None,
                label: "DC Region",
                focus_lat_deg: 39.0,
                focus_lon_deg: -77.0,
            },
        ),
        (
            "srtm_n51e000",
            OverlayPreset {
                config_path: "terrains/srtm_n51e000/config.tc.ron",
                truth_source_raster_path: None,
                label: "London",
                focus_lat_deg: 51.5,
                focus_lon_deg: 0.0,
            },
        ),
        (
            "srtm_s22w043",
            OverlayPreset {
                config_path: "terrains/srtm_s22w043/config.tc.ron",
                truth_source_raster_path: None,
                label: "Rio",
                focus_lat_deg: -22.9,
                focus_lon_deg: -43.2,
            },
        ),
        (
            "srtm_s33e151",
            OverlayPreset {
                config_path: "terrains/srtm_s33e151/config.tc.ron",
                truth_source_raster_path: None,
                label: "Sydney",
                focus_lat_deg: -33.87,
                focus_lon_deg: 151.21,
            },
        ),
    ])
}

pub fn env_f32(name: &str, default: f32) -> f32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(default)
}

pub fn env_f64(name: &str, default: f64) -> f64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(default)
}

pub fn env_bool(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        },
        Err(_) => default,
    }
}

pub fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

pub fn env_u32(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

pub fn terrain_debug_from_env() -> DebugTerrain {
    let defaults = DebugTerrain::default();
    DebugTerrain {
        lighting: env_bool(TERRAIN_LIGHTING_ENV, defaults.lighting),
        morph: env_bool(TERRAIN_MORPH_ENV, defaults.morph),
        blend: env_bool(TERRAIN_BLEND_ENV, defaults.blend),
        sample_grad: env_bool(TERRAIN_SAMPLE_GRAD_ENV, defaults.sample_grad),
        high_precision: env_bool(TERRAIN_HIGH_PRECISION_ENV, defaults.high_precision),
        ..defaults
    }
}

fn sample_source_raster_wgs84(source_raster: &Path, lat_deg: f64, lon_deg: f64) -> Option<f32> {
    let output = Command::new("gdallocationinfo")
        .args([
            "-valonly",
            "-r",
            "bilinear",
            "-wgs84",
            &source_raster.display().to_string(),
            &format!("{lon_deg:.12}"),
            &format!("{lat_deg:.12}"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f32>()
        .ok()
}

fn build_demo_drone_orbit(preset: OverlayPreset) -> Option<DemoDroneOrbit> {
    if !env_bool(ENABLE_DRONE_ENV, !benchmark_mode_enabled()) {
        return None;
    }

    let truth_source = preset.truth_source_raster_path?;
    let source_raster = Path::new(truth_source);
    if !source_raster.is_file() {
        warn!(
            "Drone demo skipped for {}: truth source raster missing at {}",
            preset.label,
            source_raster.display()
        );
        return None;
    }

    let orbit_radius_m = env_f64(DRONE_RADIUS_ENV, 1_500.0).max(100.0);
    let commanded_agl_m = env_f32(DRONE_AGL_ENV, 250.0).max(25.0);
    let sample_count = env_usize(DRONE_SAMPLES_ENV, 96).max(16);
    let period_s = env_f64(DRONE_PERIOD_ENV, 18.0).max(2.0);

    let origin = LlaHae {
        lat_deg: preset.focus_lat_deg,
        lon_deg: preset.focus_lon_deg,
        hae_m: 0.0,
    };

    let mut samples = Vec::with_capacity(sample_count);
    for sample_index in 0..sample_count {
        let theta = std::f64::consts::TAU * sample_index as f64 / sample_count as f64;
        let ned = Ned {
            n_m: orbit_radius_m * theta.cos(),
            e_m: orbit_radius_m * theta.sin(),
            d_m: 0.0,
        };
        let lla = ecef_to_lla_hae(ned_to_ecef(ned, origin));
        let Some(ground_msl_m) =
            sample_source_raster_wgs84(source_raster, lla.lat_deg, lla.lon_deg)
        else {
            continue;
        };

        let vehicle_height_m = ground_msl_m + commanded_agl_m;
        let local_position = Coordinate::from_lat_lon_degrees(lla.lat_deg, lla.lon_deg)
            .local_position(TerrainShape::WGS84, vehicle_height_m);
        samples.push(DroneOrbitSample { local_position });
    }

    if samples.len() < 8 {
        warn!(
            "Drone demo skipped for {}: only {}/{} valid orbit samples from {}",
            preset.label,
            samples.len(),
            sample_count,
            source_raster.display()
        );
        return None;
    }

    info!(
        target: "perf",
        "drone demo enabled: label={} source={} orbit_radius_m={:.1} agl_m={:.1} period_s={:.1} samples={}",
        preset.label,
        source_raster.display(),
        orbit_radius_m,
        commanded_agl_m,
        period_s,
        samples.len()
    );

    Some(DemoDroneOrbit {
        samples,
        elapsed_s: 0.0,
        period_s,
    })
}

pub fn benchmark_mode_enabled() -> bool {
    env::var(BENCHMARK_OUTPUT_ENV)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

pub fn present_mode_from_env() -> PresentMode {
    match env::var(PRESENT_MODE_ENV)
        .unwrap_or_else(|_| "auto_vsync".to_string())
        .to_lowercase()
        .as_str()
    {
        "novsync" | "auto_novsync" | "auto-no-vsync" => PresentMode::AutoNoVsync,
        "fifo" => PresentMode::Fifo,
        "fifo_relaxed" | "fifo-relaxed" => PresentMode::FifoRelaxed,
        "immediate" => PresentMode::Immediate,
        "mailbox" => PresentMode::Mailbox,
        _ => PresentMode::AutoVsync,
    }
}

pub fn benchmark_config_from_env() -> Option<BenchmarkConfig> {
    let output = env::var(BENCHMARK_OUTPUT_ENV).ok()?;
    let output = output.trim();
    if output.is_empty() {
        return None;
    }

    let warmup_s = env_f64(BENCHMARK_WARMUP_ENV, 8.0).max(0.0);
    let duration_s = env_f64(BENCHMARK_DURATION_ENV, 20.0).max(1.0);
    let ready_timeout_s = env_f64(BENCHMARK_READY_TIMEOUT_ENV, 120.0).max(1.0);
    let scenario_name = env::var(BENCHMARK_SCENARIO_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "unnamed".to_string());

    Some(BenchmarkConfig {
        output_path: PathBuf::from(output),
        scenario_name,
        warmup_s,
        duration_s,
        ready_timeout_s,
    })
}

#[cfg(feature = "metal_capture")]
pub fn metal_capture_config_from_env() -> Option<MetalCaptureConfig> {
    let frame = env::var(METAL_CAPTURE_FRAME_ENV)
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let output_dir = env::var(METAL_CAPTURE_DIR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("captures"));
    let label = env::var(BENCHMARK_SCENARIO_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "metal_capture".to_string());

    Some(MetalCaptureConfig {
        frame,
        output_dir,
        label,
    })
}

fn capture_config_from_env() -> Option<CaptureConfig> {
    let output_dir = env::var(CAPTURE_DIR_ENV).ok()?;
    let output_dir = output_dir.trim();
    if output_dir.is_empty() {
        return None;
    }

    let capture_frames_raw =
        env::var(CAPTURE_FRAMES_ENV).unwrap_or_else(|_| "120,360,720".to_string());
    let mut capture_frames = capture_frames_raw
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed.parse::<u32>().ok()
        })
        .collect::<Vec<_>>();
    capture_frames.sort_unstable();
    capture_frames.dedup();
    if capture_frames.is_empty() {
        capture_frames.push(120);
    }

    Some(CaptureConfig {
        output_dir: PathBuf::from(output_dir),
        capture_frames,
    })
}

fn camera_transform_for_focus(
    lat_deg: f64,
    lon_deg: f64,
    altitude_m: f32,
    backoff_m: f32,
) -> Transform {
    let n = unit_from_lat_lon_degrees(lat_deg, lon_deg)
        .as_vec3()
        .normalize();
    let target = n * RADIUS as f32;

    let mut east = Vec3::Y.cross(n);
    if east.length_squared() < 1e-6 {
        east = Vec3::Z.cross(n);
    }
    east = east.normalize();
    let north = n.cross(east).normalize();

    let camera_position = target + n * altitude_m + north * backoff_m + east * (0.25 * backoff_m);

    Transform::from_translation(camera_position).looking_at(target, n)
}

fn selected_overlay_keys() -> Vec<String> {
    match env::var(OVERLAY_ENV) {
        Ok(value) => {
            if value.trim().is_empty() || value.trim().eq_ignore_ascii_case("none") {
                return Vec::new();
            }
            let mut seen = std::collections::HashSet::new();
            let mut keys = Vec::new();
            for part in value.split(',') {
                let key = part.trim().to_lowercase();
                if key.is_empty() {
                    continue;
                }
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
            keys
        }
        Err(_) => DEFAULT_OVERLAY_KEYS.iter().map(|s| s.to_string()).collect(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn initialize(
    mut commands: Commands,
    mut images: ResMut<LoadingImages>,
    asset_server: Res<AssetServer>,
    click_readout: Res<ClickReadoutState>,
    mode: Res<RuntimeMode>,
    perf_telemetry: Res<TerrainPerfTelemetry>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
) {
    let overlay_map = overlay_config_map();
    let selected_keys = selected_overlay_keys();

    let benchmark_config = benchmark_config_from_env();
    let benchmark_enabled = benchmark_config.is_some();
    perf_telemetry.set_enabled(benchmark_enabled);
    if benchmark_enabled {
        perf_telemetry.reset();
    }
    if let Some(config) = benchmark_config {
        info!(
            target: "perf",
            "benchmark enabled: output={} warmup_s={:.1} duration_s={:.1} ready_timeout_s={:.1}",
            config.output_path.display(),
            config.warmup_s,
            config.duration_s,
            config.ready_timeout_s
        );
        commands.insert_resource(config);
    }
    if let Some(config) = capture_config_from_env() {
        info!(
            target: "perf",
            "screenshot capture enabled: dir={} frames={:?}",
            config.output_dir.display(),
            config.capture_frames
        );
        commands.insert_resource(config);
    }

    let focus_preset = selected_keys
        .iter()
        .find_map(|key| overlay_map.get(key.as_str()).copied())
        .or_else(|| overlay_map.get(DEFAULT_OVERLAY_KEYS[0]).copied());

    let camera_altitude_m = env_f32(CAMERA_ALT_ENV, 90_000.0);
    let camera_backoff_m = env_f32(CAMERA_BACKOFF_ENV, 150_000.0);
    if benchmark_enabled {
        if let Some(preset) = focus_preset {
            let sweep_deg = env_f64(BENCHMARK_SWEEP_DEG_ENV, 8.0).max(0.0);
            let period_s = env_f64(BENCHMARK_SWEEP_PERIOD_ENV, 40.0).max(2.0);
            commands.insert_resource(BenchmarkCameraMotion {
                center_lat_deg: preset.focus_lat_deg,
                center_lon_deg: preset.focus_lon_deg,
                altitude_m: camera_altitude_m,
                backoff_m: camera_backoff_m,
                sweep_deg,
                period_s,
                elapsed_s: 0.0,
            });
            info!(
                target: "perf",
                "benchmark camera sweep enabled: center=({:.4},{:.4}) sweep_deg={:.2} period_s={:.1}",
                preset.focus_lat_deg,
                preset.focus_lon_deg,
                sweep_deg,
                period_s
            );
        }
    }

    info!(
        target: "perf",
        "runtime_mode benchmark_mode={} debug_tools_enabled={} click_readout_enabled={} perf_title_enabled={}",
        mode.benchmark_mode,
        mode.debug_tools_enabled,
        mode.click_readout_enabled,
        mode.perf_title_enabled
    );

    if !mode.debug_tools_enabled {
        commands.spawn((
            DirectionalLight {
                illuminance: 5000.0,
                ..default()
            },
            Transform::from_xyz(-1.0, 1.0, -3.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
        commands.insert_resource(GlobalAmbientLight {
            brightness: 100.0,
            ..default()
        });
    }

    if mode.click_readout_enabled {
        commands.spawn((
            Text::new(click_readout.text()),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(12.0),
                left: Val::Px(12.0),
                ..default()
            },
            ClickReadoutText,
        ));
    }

    let camera_transform = if let Some(preset) = focus_preset {
        commands.insert_resource(CameraFocus {
            label: preset.label.to_string(),
            lat_deg: preset.focus_lat_deg,
            lon_deg: preset.focus_lon_deg,
        });

        camera_transform_for_focus(
            preset.focus_lat_deg,
            preset.focus_lon_deg,
            camera_altitude_m,
            camera_backoff_m,
        )
    } else {
        Transform::from_translation(-Vec3::X * RADIUS as f32 * 3.0).looking_to(Vec3::X, Vec3::Y)
    };

    let demo_drone_orbit = focus_preset.and_then(build_demo_drone_orbit);
    if let Some(drone_orbit) = demo_drone_orbit.clone() {
        commands.insert_resource(drone_orbit);
    }
    let msaa_samples = env_u32(MSAA_SAMPLES_ENV, 4);
    let click_marker_material = mode.click_readout_enabled.then(|| {
        standard_materials.add(StandardMaterial {
            base_color: Color::srgb(0.98, 0.92, 0.12),
            emissive: LinearRgba::rgb(6.0, 5.2, 0.8),
            unlit: true,
            ..default()
        })
    });
    let click_marker_mesh = mode
        .click_readout_enabled
        .then(|| meshes.add(Sphere::new(CLICK_MARKER_RADIUS_M).mesh().ico(5).unwrap()));

    let mut view = Entity::PLACEHOLDER;
    commands.spawn_big_space(Grid::default(), |root| {
        view = root
            .spawn_spatial((
                camera_transform,
                PrimaryTerrainCamera,
                Msaa::from_samples(msaa_samples),
                DebugCameraController::new(RADIUS),
                OrbitalCameraController::default(),
            ))
            .id();

        if let (Some(click_marker_mesh), Some(click_marker_material)) =
            (click_marker_mesh.as_ref(), click_marker_material.as_ref())
        {
            root.spawn_spatial((
                CellCoord::default(),
                Transform::from_translation(Vec3::ZERO),
                Visibility::Hidden,
                ClickMarker,
                Mesh3d(click_marker_mesh.clone()),
                MeshMaterial3d(click_marker_material.clone()),
            ));
        }

        if let Some(drone_orbit) = demo_drone_orbit.as_ref() {
            let grid = Grid::default();
            let drone_radius_m = env_f32(DRONE_SIZE_ENV, 180.0).max(25.0);
            let trail_radius_m = (0.45 * drone_radius_m).max(12.0);

            let drone_material = standard_materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 0.45, 0.05),
                emissive: LinearRgba::rgb(4.0, 1.2, 0.2),
                unlit: true,
                ..default()
            });
            let trail_material = standard_materials.add(StandardMaterial {
                base_color: Color::srgba(0.15, 0.95, 1.0, 0.55),
                emissive: LinearRgba::rgb(0.2, 1.0, 1.4),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                ..default()
            });

            for sample in drone_orbit
                .samples
                .iter()
                .step_by((drone_orbit.samples.len() / 20).max(1))
            {
                let (marker_cell, marker_translation) =
                    grid.translation_to_grid(sample.local_position);
                root.spawn_spatial((
                    marker_cell,
                    Transform::from_translation(marker_translation),
                    Mesh3d(meshes.add(Sphere::new(trail_radius_m).mesh().ico(4).unwrap())),
                    MeshMaterial3d(trail_material.clone()),
                ));
            }

            if let Some(first_sample) = drone_orbit.samples.first() {
                let (drone_cell, drone_translation) =
                    grid.translation_to_grid(first_sample.local_position);
                root.spawn_spatial((
                    drone_cell,
                    Transform::from_translation(drone_translation),
                    DemoDrone,
                    Mesh3d(meshes.add(Sphere::new(drone_radius_m).mesh().ico(5).unwrap())),
                    MeshMaterial3d(drone_material),
                ));
            }
        }
    });

    if super::asset_exists(BASE_TERRAIN_CONFIG) {
        if !super::asset_dir_exists("terrains/earth/albedo") {
            info!("Base Earth albedo not found; using height-gradient coloring.");
        }

        commands.spawn_terrain(
            asset_server.load(BASE_TERRAIN_CONFIG),
            TerrainViewConfig::default(),
            SimpleTerrainMaterial::for_terrain(&asset_server, &mut images, "terrains/earth"),
            view,
        );
    } else {
        warn!(
            "Base terrain config missing at '{}'. Restore the repo starter assets or run `./scripts/setup_earth_quickstart.sh`.",
            BASE_TERRAIN_CONFIG
        );
    }

    let mut loaded_overlays = 0_u32;
    for key in &selected_keys {
        let Some(&preset) = overlay_map.get(key.as_str()) else {
            warn!("Unknown overlay key '{key}', skipping.");
            continue;
        };

        if !super::asset_exists(preset.config_path) {
            warn!(
                "Overlay config missing at '{}', skipping.",
                preset.config_path
            );
            continue;
        }

        commands.spawn_terrain(
            asset_server.load(preset.config_path),
            TerrainViewConfig {
                order: 1,
                ..default()
            },
            SimpleTerrainMaterial::height_gradient(&asset_server, &mut images),
            view,
        );
        loaded_overlays += 1;
    }

    info!("Base terrain: {BASE_TERRAIN_CONFIG}");
    info!("Overlay selection from {OVERLAY_ENV}: {:?}", selected_keys);
    info!("Loaded overlays: {loaded_overlays}");
    if let Some(preset) = focus_preset {
        info!(
            "Camera focus: {} at lat={:.4}, lon={:.4}, alt={}m, backoff={}m",
            preset.label,
            preset.focus_lat_deg,
            preset.focus_lon_deg,
            camera_altitude_m,
            camera_backoff_m,
        );
    }
}

pub fn animate_benchmark_camera(
    time: Res<Time<Real>>,
    grids: Grids,
    motion: Option<ResMut<BenchmarkCameraMotion>>,
    mut cameras: Query<(Entity, &mut Transform, &mut CellCoord), With<PrimaryTerrainCamera>>,
) {
    let Some(mut motion) = motion else {
        return;
    };
    if motion.sweep_deg <= f64::EPSILON {
        return;
    }

    motion.elapsed_s += time.delta_secs_f64();
    let phase = std::f64::consts::TAU * (motion.elapsed_s / motion.period_s);

    let lat_deg = motion.center_lat_deg + (0.55 * motion.sweep_deg) * (phase * 0.73).sin();
    let lon_deg = motion.center_lon_deg + motion.sweep_deg * phase.cos();
    let desired = camera_transform_for_focus(lat_deg, lon_deg, motion.altitude_m, motion.backoff_m);

    for (entity, mut camera_transform, mut camera_cell) in &mut cameras {
        let Some(grid) = grids.parent_grid(entity) else {
            continue;
        };
        let (new_cell, new_translation) = grid.translation_to_grid(desired.translation.as_dvec3());
        *camera_cell = new_cell;
        camera_transform.translation = new_translation;
        camera_transform.rotation = desired.rotation;
    }
}

pub fn animate_demo_drone(
    time: Res<Time<Real>>,
    grids: Grids,
    orbit: Option<ResMut<DemoDroneOrbit>>,
    mut drones: Query<(Entity, &mut Transform, &mut CellCoord), With<DemoDrone>>,
) {
    let Some(mut orbit) = orbit else {
        return;
    };
    if orbit.samples.len() < 2 {
        return;
    }

    orbit.elapsed_s += time.delta_secs_f64();
    let phase = (orbit.elapsed_s / orbit.period_s).rem_euclid(1.0);
    let sample_f = phase * orbit.samples.len() as f64;
    let start_index = sample_f.floor() as usize % orbit.samples.len();
    let end_index = (start_index + 1) % orbit.samples.len();
    let t = sample_f.fract();

    let start = orbit.samples[start_index].local_position;
    let end = orbit.samples[end_index].local_position;
    let local_position = start.lerp(end, t);

    for (entity, mut transform, mut cell) in &mut drones {
        let Some(grid) = grids.parent_grid(entity) else {
            continue;
        };
        let (new_cell, new_translation) = grid.translation_to_grid(local_position);
        *cell = new_cell;
        transform.translation = new_translation;
    }
}

pub fn finish_loading_images_local(
    asset_server: Res<AssetServer>,
    mut loading_images: ResMut<LoadingImages>,
    mut images: ResMut<Assets<Image>>,
) {
    loading_images.finalize_ready_images(&asset_server, &mut images);
}
