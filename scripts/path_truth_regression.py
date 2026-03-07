#!/usr/bin/env python3
"""
Run an end-to-end local-frame path regression against rendered terrain.

The horizontal path geometry is generated from a local N/E orbit around an origin using WGS84
NED->ECEF->LLA math. Each orbit point is then assigned an altitude from a chosen truth ground
model plus a commanded AGL offset. The resulting metric answers the operational question:

  if a path is positioned from the chosen truth model, how far from the rendered terrain does the
  path appear to be along the orbit?

Metrics:
  rendered_agl_error_m = (vehicle_msl - renderer_ground_msl) - commanded_agl_m

When `--truth-ground source_raster`, this is the renderer-vs-source residual sampled along the
path. When `--truth-ground small_world`, this is the expected AGL error for small_world-anchored
path data rendered against the current terrain surface.
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable, Dict, List, Optional, Tuple

import compare_small_world_ground as ground_compare
from compare_renderer_to_source_raster import sample_source_raster_wgs84

WGS84_A = 6_378_137.0
WGS84_B = 6_356_752.314_245_18
WGS84_E2 = 0.006_694_379_990_141_33
WGS84_EP2 = WGS84_E2 / (1.0 - WGS84_E2)


@dataclass
class OrbitSummary:
    truth_ground: str
    origin_lat_deg: float
    origin_lon_deg: float
    radius_m: float
    commanded_agl_m: float
    sample_count: int
    compared_count: int
    renderer_sample_lod: int
    terrain_root: str
    source_raster_path: str
    hgt_root: Optional[str]
    center_vehicle_msl_m: Optional[float]
    center_renderer_ground_msl_m: Optional[float]
    center_source_ground_msl_m: Optional[float]
    center_small_world_ground_msl_m: Optional[float]
    center_rendered_agl_error_m: Optional[float]
    rendered_agl_error_mean_m: float
    rendered_agl_error_rms_m: float
    rendered_agl_error_abs_p50_m: float
    rendered_agl_error_abs_p95_m: float
    rendered_agl_error_abs_max_m: float
    source_agl_error_mean_m: Optional[float]
    source_agl_error_abs_p95_m: Optional[float]
    small_world_agl_error_mean_m: Optional[float]
    small_world_agl_error_abs_p95_m: Optional[float]


def percentile_abs(values: List[float], fraction: float) -> float:
    return ground_compare.percentile([abs(v) for v in values], fraction)


def lla_to_ecef(lat_deg: float, lon_deg: float, hae_m: float) -> Tuple[float, float, float]:
    lat = math.radians(lat_deg)
    lon = math.radians(lon_deg)
    s_lat = math.sin(lat)
    c_lat = math.cos(lat)
    s_lon = math.sin(lon)
    c_lon = math.cos(lon)
    nu = WGS84_A / math.sqrt(1.0 - WGS84_E2 * s_lat * s_lat)
    return (
        (nu + hae_m) * c_lat * c_lon,
        (nu + hae_m) * c_lat * s_lon,
        (nu * (1.0 - WGS84_E2) + hae_m) * s_lat,
    )


def ecef_to_lla(x_m: float, y_m: float, z_m: float) -> Tuple[float, float, float]:
    p = math.sqrt(x_m * x_m + y_m * y_m)
    q = math.atan2(z_m * WGS84_A, p * WGS84_B)
    sin_q = math.sin(q)
    cos_q = math.cos(q)
    lat = math.atan2(
        z_m + WGS84_EP2 * WGS84_B * sin_q * sin_q * sin_q,
        p - WGS84_E2 * WGS84_A * cos_q * cos_q * cos_q,
    )
    lon = math.atan2(y_m, x_m)
    nu = WGS84_A / math.sqrt(1.0 - WGS84_E2 * math.sin(lat) ** 2)
    hae = p / math.cos(lat) - nu
    return math.degrees(lat), math.degrees(lon), hae


def geo_conversion_params(origin_lat_deg: float, origin_lon_deg: float, origin_hae_m: float) -> Tuple[List[List[float]], float, float, float]:
    lat0 = math.radians(origin_lat_deg)
    lon0 = math.radians(origin_lon_deg)
    s_lat0 = math.sin(lat0)
    c_lat0 = math.cos(lat0)
    s_lon0 = math.sin(lon0)
    c_lon0 = math.cos(lon0)
    nu0 = WGS84_A / math.sqrt(1.0 - WGS84_E2 * s_lat0 * s_lat0)

    rot = [
        [-s_lat0 * c_lon0, -s_lat0 * s_lon0, c_lat0],
        [-s_lon0, c_lon0, 0.0],
        [-c_lat0 * c_lon0, -c_lat0 * s_lon0, -s_lat0],
    ]

    x0 = (nu0 + origin_hae_m) * c_lat0 * c_lon0
    y0 = (nu0 + origin_hae_m) * c_lat0 * s_lon0
    z0 = (nu0 * (1.0 - WGS84_E2) + origin_hae_m) * s_lat0
    return rot, x0, y0, z0


def ned_to_lla(n_m: float, e_m: float, d_m: float, origin_lat_deg: float, origin_lon_deg: float, origin_hae_m: float = 0.0) -> Tuple[float, float, float]:
    rot, x0, y0, z0 = geo_conversion_params(origin_lat_deg, origin_lon_deg, origin_hae_m)
    dx = rot[0][0] * n_m + rot[1][0] * e_m + rot[2][0] * d_m
    dy = rot[0][1] * n_m + rot[1][1] * e_m + rot[2][1] * d_m
    dz = rot[0][2] * n_m + rot[1][2] * e_m + rot[2][2] * d_m
    return ecef_to_lla(dx + x0, dy + y0, dz + z0)


def source_ground_sampler(source_raster: Path) -> Callable[[float, float], Optional[float]]:
    return lambda lat_deg, lon_deg: sample_source_raster_wgs84(source_raster, lat_deg, lon_deg)


def small_world_ground_sampler(hgt_sampler: ground_compare.HgtSampler) -> Callable[[float, float], Optional[float]]:
    return lambda lat_deg, lon_deg: hgt_sampler.sample_bilinear(lat_deg, lon_deg)


def optional_stats(values: List[float]) -> Tuple[Optional[float], Optional[float]]:
    if not values:
        return None, None
    return ground_compare.mean(values), percentile_abs(values, 0.95)


def main() -> int:
    project_root = Path(__file__).resolve().parents[1]

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--origin-lat", type=float, required=True)
    parser.add_argument("--origin-lon", type=float, required=True)
    parser.add_argument("--radius-m", type=float, default=1000.0)
    parser.add_argument("--commanded-agl-m", type=float, default=100.0)
    parser.add_argument("--sample-count", type=int, default=64)
    parser.add_argument(
        "--truth-ground",
        choices=("source_raster", "small_world"),
        default="small_world",
    )
    parser.add_argument(
        "--terrain-root",
        type=Path,
        default=project_root / "assets" / "terrains" / "earth",
    )
    parser.add_argument("--source-raster", type=Path, required=True)
    parser.add_argument("--hgt-root", type=Path, default=None)
    parser.add_argument("--renderer-sample-lod", type=int, default=None)
    parser.add_argument("--json-out", type=Path, default=None)
    args = parser.parse_args()

    if args.truth_ground == "small_world":
        sibling_default = project_root.parent / "planetary_test" / "data" / "srtm"
        if args.hgt_root is not None:
            hgt_root = args.hgt_root
        elif sibling_default.is_dir():
            hgt_root = sibling_default
        else:
            hgt_root = ground_compare.resolve_hgt_root(project_root, None)
        hgt_sampler = ground_compare.HgtSampler(hgt_root)
        truth_sampler = small_world_ground_sampler(hgt_sampler)
        hgt_root_str: Optional[str] = str(hgt_root.resolve())
    else:
        hgt_sampler = None
        truth_sampler = source_ground_sampler(args.source_raster)
        hgt_root_str = None

    renderer_sample_lod = ground_compare.resolve_renderer_sample_lod(
        args.terrain_root, args.renderer_sample_lod
    )

    rendered_agl_errors: List[float] = []
    source_agl_errors: List[float] = []
    small_world_agl_errors: List[float] = []
    compared_count = 0

    center_vehicle_msl = None
    center_renderer_ground = None
    center_source_ground = None
    center_small_world_ground = None
    center_rendered_agl_error = None

    for sample_index in range(args.sample_count):
        theta = math.tau * sample_index / args.sample_count
        north_m = args.radius_m * math.cos(theta)
        east_m = args.radius_m * math.sin(theta)
        lat_deg, lon_deg, _ = ned_to_lla(
            north_m,
            east_m,
            0.0,
            args.origin_lat,
            args.origin_lon,
            0.0,
        )

        truth_ground = truth_sampler(lat_deg, lon_deg)
        if truth_ground is None:
            continue

        renderer_ground = ground_compare.sample_renderer_ground_msl(
            args.terrain_root, lat_deg, lon_deg, renderer_sample_lod
        )
        source_ground = sample_source_raster_wgs84(args.source_raster, lat_deg, lon_deg)
        small_world_ground = (
            None if hgt_sampler is None else hgt_sampler.sample_bilinear(lat_deg, lon_deg)
        )

        if renderer_ground is None:
            continue

        vehicle_msl = truth_ground + args.commanded_agl_m
        rendered_agl_error = (vehicle_msl - renderer_ground) - args.commanded_agl_m

        rendered_agl_errors.append(rendered_agl_error)
        if source_ground is not None:
            source_agl_errors.append((vehicle_msl - source_ground) - args.commanded_agl_m)
        if small_world_ground is not None:
            small_world_agl_errors.append((vehicle_msl - small_world_ground) - args.commanded_agl_m)
        compared_count += 1

        if sample_index == 0:
            center_vehicle_msl = vehicle_msl
            center_renderer_ground = renderer_ground
            center_source_ground = source_ground
            center_small_world_ground = small_world_ground
            center_rendered_agl_error = rendered_agl_error

    if not rendered_agl_errors:
        raise RuntimeError("No comparable orbit samples found")

    source_mean, source_p95 = optional_stats(source_agl_errors)
    small_world_mean, small_world_p95 = optional_stats(small_world_agl_errors)

    summary = OrbitSummary(
        truth_ground=args.truth_ground,
        origin_lat_deg=args.origin_lat,
        origin_lon_deg=args.origin_lon,
        radius_m=args.radius_m,
        commanded_agl_m=args.commanded_agl_m,
        sample_count=args.sample_count,
        compared_count=compared_count,
        renderer_sample_lod=renderer_sample_lod,
        terrain_root=str(args.terrain_root.resolve()),
        source_raster_path=str(args.source_raster.resolve()),
        hgt_root=hgt_root_str,
        center_vehicle_msl_m=center_vehicle_msl,
        center_renderer_ground_msl_m=center_renderer_ground,
        center_source_ground_msl_m=center_source_ground,
        center_small_world_ground_msl_m=center_small_world_ground,
        center_rendered_agl_error_m=center_rendered_agl_error,
        rendered_agl_error_mean_m=ground_compare.mean(rendered_agl_errors),
        rendered_agl_error_rms_m=ground_compare.rms(rendered_agl_errors),
        rendered_agl_error_abs_p50_m=percentile_abs(rendered_agl_errors, 0.50),
        rendered_agl_error_abs_p95_m=percentile_abs(rendered_agl_errors, 0.95),
        rendered_agl_error_abs_max_m=max(abs(v) for v in rendered_agl_errors),
        source_agl_error_mean_m=source_mean,
        source_agl_error_abs_p95_m=source_p95,
        small_world_agl_error_mean_m=small_world_mean,
        small_world_agl_error_abs_p95_m=small_world_p95,
    )

    print(f"Truth ground:                        {summary.truth_ground}")
    print(
        f"Origin:                              lat={summary.origin_lat_deg:.6f}, lon={summary.origin_lon_deg:.6f}"
    )
    print(
        f"Orbit:                               radius={summary.radius_m:.1f} m, commanded_agl={summary.commanded_agl_m:.1f} m, samples={summary.sample_count}"
    )
    print(f"Compared points:                     {summary.compared_count}/{summary.sample_count}")
    print(f"Renderer terrain root:               {summary.terrain_root}")
    print(f"Renderer sample LOD:                 {summary.renderer_sample_lod}")
    print(f"Source raster:                       {summary.source_raster_path}")
    if summary.hgt_root is not None:
        print(f"small_world HGT root:                {summary.hgt_root}")
    print(
        f"Center rendered_agl_error_m:         {summary.center_rendered_agl_error_m:.3f}"
        if summary.center_rendered_agl_error_m is not None
        else "Center rendered_agl_error_m:         n/a"
    )
    print("Path stats:")
    print(f"  rendered_agl_error_m mean:         {summary.rendered_agl_error_mean_m:.3f}")
    print(f"  rendered_agl_error_m rms:          {summary.rendered_agl_error_rms_m:.3f}")
    print(f"  rendered_agl_error_abs p50:        {summary.rendered_agl_error_abs_p50_m:.3f}")
    print(f"  rendered_agl_error_abs p95:        {summary.rendered_agl_error_abs_p95_m:.3f}")
    print(f"  rendered_agl_error_abs max:        {summary.rendered_agl_error_abs_max_m:.3f}")
    if summary.source_agl_error_mean_m is not None:
        print(f"  source_agl_error_m mean:           {summary.source_agl_error_mean_m:.3f}")
        print(f"  source_agl_error_abs p95:          {summary.source_agl_error_abs_p95_m:.3f}")
    if summary.small_world_agl_error_mean_m is not None:
        print(f"  small_world_agl_error_m mean:      {summary.small_world_agl_error_mean_m:.3f}")
        print(f"  small_world_agl_error_abs p95:     {summary.small_world_agl_error_abs_p95_m:.3f}")

    if args.json_out is not None:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(json.dumps(asdict(summary), indent=2) + "\n", encoding="utf-8")
        print(f"JSON summary:                        {args.json_out}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
