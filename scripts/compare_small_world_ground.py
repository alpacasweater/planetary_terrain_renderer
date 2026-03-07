#!/usr/bin/env python3
"""
Compare bevy_terrain ground samples against small_world-style HGT ground samples.

Metrics:
  ground_model_delta_m = renderer_ground_msl - small_world_ground_msl
  expected_agl_error_m = -ground_model_delta_m

`expected_agl_error_m` answers the practical question: if a vehicle/path position is
computed from small_world ground truth but rendered against the current terrain surface,
how far off from the rendered ground should you expect it to be?
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import struct
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple


SIGMA = 0.87 * 0.87
DEFAULT_RENDERER_SAMPLE_LOD = 3
RENDERER_TEXTURE_SIZE = 512.0
RENDERER_BORDER = 4.0
RENDERER_CENTER = RENDERER_TEXTURE_SIZE - 2.0 * RENDERER_BORDER
RENDERER_TILE_BLOCK_SIZE = 8
WGS84_A = 6_378_137.0
WGS84_B = 6_356_752.314_245_18
WGS84_E2 = 1.0 - (WGS84_B * WGS84_B) / (WGS84_A * WGS84_A)


@dataclass
class RendererSample:
    face: int
    tile_x: int
    tile_y: int
    pixel_x: float
    pixel_y: float


@dataclass
class Summary:
    lat_deg: float
    lon_deg: float
    grid_half_size_m: float
    grid_step_m: float
    point_count: int
    compared_count: int
    renderer_sample_lod: int
    renderer_terrain_root: str
    small_world_hgt_root: str
    center_renderer_ground_msl_m: Optional[float]
    center_small_world_ground_msl_m: Optional[float]
    center_ground_model_delta_m: Optional[float]
    center_expected_agl_error_m: Optional[float]
    ground_model_delta_m_mean: float
    ground_model_delta_m_min: float
    ground_model_delta_m_max: float
    ground_model_delta_m_rms: float
    ground_model_delta_abs_p50_m: float
    ground_model_delta_abs_p95_m: float
    ground_model_delta_abs_max_m: float
    expected_agl_error_m_mean: float
    expected_agl_error_m_min: float
    expected_agl_error_m_max: float
    expected_agl_error_m_rms: float
    expected_agl_error_abs_p50_m: float
    expected_agl_error_abs_p95_m: float
    expected_agl_error_abs_max_m: float


class HgtSampler:
    def __init__(self, root: Path):
        self.root = root
        self.cache: Dict[str, Tuple[int, List[int]]] = {}

    @staticmethod
    def tile_name(lat_deg: float, lon_deg: float) -> str:
        lat_floor = math.floor(lat_deg)
        lon_floor = math.floor(lon_deg)
        lat_prefix = "N" if lat_floor >= 0 else "S"
        lon_prefix = "E" if lon_floor >= 0 else "W"
        return f"{lat_prefix}{abs(lat_floor):02d}{lon_prefix}{abs(lon_floor):03d}.hgt"

    def _load_tile(self, tile_name: str) -> Optional[Tuple[int, List[int]]]:
        if tile_name in self.cache:
            return self.cache[tile_name]

        tile_path = self.root / tile_name
        if not tile_path.is_file():
            return None

        size_bytes = tile_path.stat().st_size
        sample_count = size_bytes // 2
        side = int(round(math.sqrt(sample_count)))
        if side * side != sample_count:
            raise RuntimeError(f"Unexpected HGT tile size for {tile_path} ({size_bytes} bytes)")

        with tile_path.open("rb") as handle:
            values = list(struct.unpack(f">{sample_count}h", handle.read()))

        self.cache[tile_name] = (side, values)
        return side, values

    def sample_bilinear(self, lat_deg: float, lon_deg: float) -> Optional[float]:
        tile = self._load_tile(self.tile_name(lat_deg, lon_deg))
        if tile is None:
            return None

        side, values = tile
        lat_floor = math.floor(lat_deg)
        lon_floor = math.floor(lon_deg)

        u = (lon_deg - lon_floor) * (side - 1)
        v = ((lat_floor + 1.0) - lat_deg) * (side - 1)

        x0 = max(0, min(side - 2, int(math.floor(u))))
        y0 = max(0, min(side - 2, int(math.floor(v))))
        tx = max(0.0, min(1.0, u - x0))
        ty = max(0.0, min(1.0, v - y0))

        def at(ix: int, iy: int) -> int:
            return values[iy * side + ix]

        p00 = at(x0, y0)
        p10 = at(x0 + 1, y0)
        p01 = at(x0, y0 + 1)
        p11 = at(x0 + 1, y0 + 1)

        top = (1.0 - tx) * p00 + tx * p10
        bottom = (1.0 - tx) * p01 + tx * p11
        return (1.0 - ty) * top + ty * bottom


def renderer_unit_from_lat_lon(lat_deg: float, lon_deg: float) -> Tuple[float, float, float]:
    lat = math.radians(lat_deg)
    lon = math.radians(lon_deg)
    sin_lat = math.sin(lat)
    cos_lat = math.cos(lat)
    sin_lon = math.sin(lon)
    cos_lon = math.cos(lon)

    n = WGS84_A / math.sqrt(1.0 - WGS84_E2 * sin_lat * sin_lat)
    ecef_x = n * cos_lat * cos_lon
    ecef_y = n * cos_lat * sin_lon
    ecef_z = n * (1.0 - WGS84_E2) * sin_lat

    x = -ecef_x / WGS84_A
    y = ecef_z / WGS84_B
    z = ecef_y / WGS84_A
    length = math.sqrt(x * x + y * y + z * z)
    return x / length, y / length, z / length


def coordinate_from_lat_lon(lat_deg: float, lon_deg: float) -> Tuple[int, float, float]:
    x, y, z = renderer_unit_from_lat_lon(lat_deg, lon_deg)

    if abs(x) > abs(y) and abs(x) > abs(z) and x < 0.0:
        face = 0
    elif abs(x) > abs(y) and abs(x) > abs(z):
        face = 3
    elif abs(z) > abs(y) and z > 0.0:
        face = 1
    elif abs(z) > abs(y):
        face = 4
    elif y > 0.0:
        face = 2
    else:
        face = 5

    inverse_face_matrices = {
        0: ((-1.0, 0.0, 0.0), (0.0, 0.0, -1.0), (0.0, 1.0, 0.0)),
        1: ((0.0, 1.0, 0.0), (0.0, 0.0, -1.0), (1.0, 0.0, 0.0)),
        2: ((0.0, 1.0, 0.0), (1.0, 0.0, 0.0), (0.0, 0.0, 1.0)),
        3: ((1.0, 0.0, 0.0), (0.0, -1.0, 0.0), (0.0, 0.0, 1.0)),
        4: ((0.0, 0.0, 1.0), (0.0, -1.0, 0.0), (-1.0, 0.0, 0.0)),
        5: ((0.0, 0.0, 1.0), (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0)),
    }
    c0, c1, c2 = inverse_face_matrices[face]
    abc_x = c0[0] * x + c1[0] * y + c2[0] * z
    abc_y = c0[1] * x + c1[1] * y + c2[1] * z
    abc_z = c0[2] * x + c1[2] * y + c2[2] * z

    xy_x = abc_y / abc_x
    xy_y = abc_z / abc_x
    uv_x = 0.5 * xy_x * math.sqrt((1.0 + SIGMA) / (1.0 + SIGMA * xy_x * xy_x)) + 0.5
    uv_y = 0.5 * xy_y * math.sqrt((1.0 + SIGMA) / (1.0 + SIGMA * xy_y * xy_y)) + 0.5
    return face, uv_x, uv_y


def resolve_renderer_sample_lod(terrain_root: Path, explicit_lod: Optional[int] = None) -> int:
    if explicit_lod is not None:
        return explicit_lod

    config_path = terrain_root / "config.tc.ron"
    if not config_path.is_file():
        return DEFAULT_RENDERER_SAMPLE_LOD

    match = re.search(r"lod_count:\s*(\d+)", config_path.read_text())
    if match is None:
        return DEFAULT_RENDERER_SAMPLE_LOD

    return max(0, int(match.group(1)) - 1)


def renderer_sample_location(
    lat_deg: float, lon_deg: float, renderer_sample_lod: int
) -> RendererSample:
    face, uv_x, uv_y = coordinate_from_lat_lon(lat_deg, lon_deg)
    tile_count = 1 << renderer_sample_lod

    tile_x = max(0, min(tile_count - 1, int(math.floor(uv_x * tile_count))))
    tile_y = max(0, min(tile_count - 1, int(math.floor(uv_y * tile_count))))

    local_u = uv_x * tile_count - tile_x
    local_v = uv_y * tile_count - tile_y
    pixel_x = RENDERER_BORDER + local_u * RENDERER_CENTER
    pixel_y = RENDERER_BORDER + local_v * RENDERER_CENTER
    return RendererSample(face, tile_x, tile_y, pixel_x, pixel_y)


def sample_renderer_ground_msl(
    terrain_root: Path, lat_deg: float, lon_deg: float, renderer_sample_lod: int
) -> Optional[float]:
    sample = renderer_sample_location(lat_deg, lon_deg, renderer_sample_lod)
    tile_block_x = sample.tile_x // RENDERER_TILE_BLOCK_SIZE
    tile_block_y = sample.tile_y // RENDERER_TILE_BLOCK_SIZE
    tile_file = (
        terrain_root
        / "height"
        / str(renderer_sample_lod)
        / f"{tile_block_x}_{tile_block_y}"
        / f"{sample.face}_{renderer_sample_lod}_{sample.tile_x}_{sample.tile_y}.tif"
    )
    if not tile_file.is_file():
        return None

    cmd = [
        "gdallocationinfo",
        "-valonly",
        "-r",
        "bilinear",
        str(tile_file),
        f"{sample.pixel_x:.6f}",
        f"{sample.pixel_y:.6f}",
    ]
    try:
        output = subprocess.check_output(cmd, stderr=subprocess.STDOUT, text=True)
    except FileNotFoundError as exc:
        raise RuntimeError("gdallocationinfo is required but was not found on PATH") from exc
    except subprocess.CalledProcessError:
        return None

    try:
        return float(output.strip())
    except ValueError:
        return None


def offset_lat_lon(lat0_deg: float, lon0_deg: float, north_m: float, east_m: float) -> Tuple[float, float]:
    meters_per_deg_lat = 111_320.0
    meters_per_deg_lon = 111_320.0 * math.cos(math.radians(lat0_deg))
    dlat = north_m / meters_per_deg_lat
    dlon = east_m / meters_per_deg_lon if abs(meters_per_deg_lon) > 1e-6 else 0.0
    return lat0_deg + dlat, lon0_deg + dlon


def percentile(values: List[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = int(round((len(ordered) - 1) * min(max(fraction, 0.0), 1.0)))
    return ordered[index]


def mean(values: List[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def rms(values: List[float]) -> float:
    return math.sqrt(sum(v * v for v in values) / len(values)) if values else 0.0


def resolve_hgt_root(project_root: Path, explicit_root: Optional[Path]) -> Path:
    if explicit_root is not None:
        return explicit_root

    env_root = os.environ.get("SMALL_WORLD_HGT_ROOT")
    if env_root:
        return Path(env_root)

    repo_default = project_root / "data" / "srtm"
    if repo_default.is_dir():
        return repo_default

    raise RuntimeError(
        "HGT root not found. Pass --hgt-root or set SMALL_WORLD_HGT_ROOT."
    )


def print_summary(summary: Summary) -> None:
    print(f"Center:                         lat={summary.lat_deg:.6f}, lon={summary.lon_deg:.6f}")
    print(
        f"Grid:                           half_size={summary.grid_half_size_m:.1f} m, "
        f"step={summary.grid_step_m:.1f} m"
    )
    print(f"Compared points:                {summary.compared_count}/{summary.point_count}")
    print(f"Renderer terrain root:          {summary.renderer_terrain_root}")
    print(f"small_world HGT root:           {summary.small_world_hgt_root}")

    if summary.center_renderer_ground_msl_m is not None and summary.center_small_world_ground_msl_m is not None:
        print(f"Center renderer ground MSL:     {summary.center_renderer_ground_msl_m:.3f} m")
        print(f"Center small_world ground MSL:  {summary.center_small_world_ground_msl_m:.3f} m")
        print(f"Center ground_model_delta_m:    {summary.center_ground_model_delta_m:.3f} m")
        print(f"Center expected_agl_error_m:    {summary.center_expected_agl_error_m:.3f} m")
    else:
        print("Center sample:                  unavailable")

    print("Grid stats:")
    print(f"  ground_model_delta_m mean:    {summary.ground_model_delta_m_mean:.3f}")
    print(f"  ground_model_delta_m min:     {summary.ground_model_delta_m_min:.3f}")
    print(f"  ground_model_delta_m max:     {summary.ground_model_delta_m_max:.3f}")
    print(f"  ground_model_delta_m rms:     {summary.ground_model_delta_m_rms:.3f}")
    print(f"  ground_model_delta_abs p50:   {summary.ground_model_delta_abs_p50_m:.3f}")
    print(f"  ground_model_delta_abs p95:   {summary.ground_model_delta_abs_p95_m:.3f}")
    print(f"  ground_model_delta_abs max:   {summary.ground_model_delta_abs_max_m:.3f}")
    print(f"  expected_agl_error_m mean:    {summary.expected_agl_error_m_mean:.3f}")
    print(f"  expected_agl_error_m min:     {summary.expected_agl_error_m_min:.3f}")
    print(f"  expected_agl_error_m max:     {summary.expected_agl_error_m_max:.3f}")
    print(f"  expected_agl_error_m rms:     {summary.expected_agl_error_m_rms:.3f}")
    print(f"  expected_agl_error_abs p50:   {summary.expected_agl_error_abs_p50_m:.3f}")
    print(f"  expected_agl_error_abs p95:   {summary.expected_agl_error_abs_p95_m:.3f}")
    print(f"  expected_agl_error_abs max:   {summary.expected_agl_error_abs_max_m:.3f}")


def main() -> int:
    project_root = Path(__file__).resolve().parents[1]

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lat", type=float, required=True, help="Center latitude in degrees")
    parser.add_argument("--lon", type=float, required=True, help="Center longitude in degrees")
    parser.add_argument("--half-size-m", type=float, default=1000.0, help="Half-width of sample square in meters")
    parser.add_argument("--step-m", type=float, default=250.0, help="Grid spacing in meters")
    parser.add_argument(
        "--terrain-root",
        type=Path,
        default=project_root / "assets" / "terrains" / "earth",
        help="Renderer terrain asset root",
    )
    parser.add_argument("--hgt-root", type=Path, default=None, help="small_world-compatible HGT root")
    parser.add_argument("--renderer-sample-lod", type=int, default=None, help="Optional explicit renderer sample lod")
    parser.add_argument("--json-out", type=Path, default=None, help="Optional machine-readable output path")
    args = parser.parse_args()

    try:
        hgt_root = resolve_hgt_root(project_root, args.hgt_root)
    except RuntimeError as error:
        print(str(error), file=sys.stderr)
        return 2

    terrain_root = args.terrain_root
    if not terrain_root.is_dir():
        print(f"Renderer terrain root missing: {terrain_root}", file=sys.stderr)
        return 2
    if not hgt_root.is_dir():
        print(f"HGT root missing: {hgt_root}", file=sys.stderr)
        return 2

    renderer_sample_lod = resolve_renderer_sample_lod(terrain_root, args.renderer_sample_lod)

    sampler = HgtSampler(hgt_root)

    n = int(round((2.0 * args.half_size_m) / args.step_m))
    offsets = [(-args.half_size_m + i * args.step_m) for i in range(n + 1)]

    deltas: List[float] = []
    agl_errors: List[float] = []
    point_count = 0
    compared_count = 0

    center_renderer = sample_renderer_ground_msl(
        terrain_root, args.lat, args.lon, renderer_sample_lod
    )
    center_small_world = sampler.sample_bilinear(args.lat, args.lon)

    for north_m in offsets:
        for east_m in offsets:
            point_count += 1
            lat_deg, lon_deg = offset_lat_lon(args.lat, args.lon, north_m, east_m)
            small_world_ground = sampler.sample_bilinear(lat_deg, lon_deg)
            renderer_ground = sample_renderer_ground_msl(
                terrain_root, lat_deg, lon_deg, renderer_sample_lod
            )
            if small_world_ground is None or renderer_ground is None:
                continue

            delta = renderer_ground - small_world_ground
            deltas.append(delta)
            agl_errors.append(-delta)
            compared_count += 1

    if not deltas:
        print("No comparable samples found.", file=sys.stderr)
        return 2

    delta_abs = [abs(value) for value in deltas]
    agl_abs = [abs(value) for value in agl_errors]
    center_delta = None if center_renderer is None or center_small_world is None else center_renderer - center_small_world

    summary = Summary(
        lat_deg=args.lat,
        lon_deg=args.lon,
        grid_half_size_m=args.half_size_m,
        grid_step_m=args.step_m,
        point_count=point_count,
        compared_count=compared_count,
        renderer_sample_lod=renderer_sample_lod,
        renderer_terrain_root=str(terrain_root),
        small_world_hgt_root=str(hgt_root),
        center_renderer_ground_msl_m=center_renderer,
        center_small_world_ground_msl_m=center_small_world,
        center_ground_model_delta_m=center_delta,
        center_expected_agl_error_m=None if center_delta is None else -center_delta,
        ground_model_delta_m_mean=mean(deltas),
        ground_model_delta_m_min=min(deltas),
        ground_model_delta_m_max=max(deltas),
        ground_model_delta_m_rms=rms(deltas),
        ground_model_delta_abs_p50_m=percentile(delta_abs, 0.50),
        ground_model_delta_abs_p95_m=percentile(delta_abs, 0.95),
        ground_model_delta_abs_max_m=max(delta_abs),
        expected_agl_error_m_mean=mean(agl_errors),
        expected_agl_error_m_min=min(agl_errors),
        expected_agl_error_m_max=max(agl_errors),
        expected_agl_error_m_rms=rms(agl_errors),
        expected_agl_error_abs_p50_m=percentile(agl_abs, 0.50),
        expected_agl_error_abs_p95_m=percentile(agl_abs, 0.95),
        expected_agl_error_abs_max_m=max(agl_abs),
    )

    print_summary(summary)

    if args.json_out is not None:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(json.dumps(asdict(summary), indent=2) + "\n", encoding="utf-8")
        print(f"JSON summary:                   {args.json_out}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
