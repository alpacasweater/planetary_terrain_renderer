#!/usr/bin/env python3
"""
Compare renderer ground samples against the original georeferenced source raster.

Metrics:
  preprocess_runtime_delta_m = renderer_ground_msl - source_raster_ground_msl

This isolates reprojection, cube-face mapping, tile splitting, downsampling, and runtime
sampling error from source-dataset disagreement with an external truth model such as small_world.
"""

from __future__ import annotations

import argparse
import json
import subprocess
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import List, Optional

import compare_small_world_ground as renderer_compare


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
    source_raster_path: str
    center_renderer_ground_msl_m: Optional[float]
    center_source_ground_msl_m: Optional[float]
    center_preprocess_runtime_delta_m: Optional[float]
    preprocess_runtime_delta_m_mean: float
    preprocess_runtime_delta_m_min: float
    preprocess_runtime_delta_m_max: float
    preprocess_runtime_delta_m_rms: float
    preprocess_runtime_delta_abs_p50_m: float
    preprocess_runtime_delta_abs_p95_m: float
    preprocess_runtime_delta_abs_max_m: float


def sample_source_raster_wgs84(source_raster: Path, lat_deg: float, lon_deg: float) -> Optional[float]:
    cmd = [
        "gdallocationinfo",
        "-valonly",
        "-r",
        "bilinear",
        "-wgs84",
        str(source_raster),
        f"{lon_deg:.12f}",
        f"{lat_deg:.12f}",
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--lat", type=float, required=True)
    parser.add_argument("--lon", type=float, required=True)
    parser.add_argument("--grid-half-size-m", type=float, default=1000.0)
    parser.add_argument("--grid-step-m", type=float, default=250.0)
    parser.add_argument("--terrain-root", type=Path, default=Path("assets/terrains/earth"))
    parser.add_argument("--source-raster", type=Path, required=True)
    parser.add_argument("--renderer-sample-lod", type=int)
    parser.add_argument("--json-out", type=Path)
    args = parser.parse_args()

    renderer_sample_lod = renderer_compare.resolve_renderer_sample_lod(
        args.terrain_root, args.renderer_sample_lod
    )

    deltas: List[float] = []
    center_renderer = renderer_compare.sample_renderer_ground_msl(
        args.terrain_root, args.lat, args.lon, renderer_sample_lod
    )
    center_source = None
    compared_count = 0
    point_count = 0

    north = -args.grid_half_size_m
    while north <= args.grid_half_size_m + 1e-6:
        east = -args.grid_half_size_m
        while east <= args.grid_half_size_m + 1e-6:
            lat_deg, lon_deg = renderer_compare.offset_lat_lon(args.lat, args.lon, north, east)
            renderer_ground = renderer_compare.sample_renderer_ground_msl(
                args.terrain_root, lat_deg, lon_deg, renderer_sample_lod
            )
            source_ground = sample_source_raster_wgs84(args.source_raster, lat_deg, lon_deg)

            point_count += 1
            if renderer_ground is not None and source_ground is not None:
                delta = renderer_ground - source_ground
                deltas.append(delta)
                compared_count += 1
                if abs(north) < 1e-9 and abs(east) < 1e-9:
                    center_renderer = renderer_ground
                    center_source = source_ground

            east += args.grid_step_m
        north += args.grid_step_m

    summary = Summary(
        lat_deg=args.lat,
        lon_deg=args.lon,
        grid_half_size_m=args.grid_half_size_m,
        grid_step_m=args.grid_step_m,
        point_count=point_count,
        compared_count=compared_count,
        renderer_sample_lod=renderer_sample_lod,
        renderer_terrain_root=str(args.terrain_root.resolve()),
        source_raster_path=str(args.source_raster.resolve()),
        center_renderer_ground_msl_m=center_renderer,
        center_source_ground_msl_m=center_source,
        center_preprocess_runtime_delta_m=(
            None if center_renderer is None or center_source is None else center_renderer - center_source
        ),
        preprocess_runtime_delta_m_mean=renderer_compare.mean(deltas),
        preprocess_runtime_delta_m_min=min(deltas) if deltas else 0.0,
        preprocess_runtime_delta_m_max=max(deltas) if deltas else 0.0,
        preprocess_runtime_delta_m_rms=renderer_compare.rms(deltas),
        preprocess_runtime_delta_abs_p50_m=renderer_compare.percentile([abs(v) for v in deltas], 0.50),
        preprocess_runtime_delta_abs_p95_m=renderer_compare.percentile([abs(v) for v in deltas], 0.95),
        preprocess_runtime_delta_abs_max_m=max((abs(v) for v in deltas), default=0.0),
    )

    print(f"Center:                               lat={args.lat:.6f}, lon={args.lon:.6f}")
    print(f"Grid:                                 half_size={args.grid_half_size_m:.1f} m, step={args.grid_step_m:.1f} m")
    print(f"Compared points:                      {compared_count}/{point_count}")
    print(f"Renderer terrain root:                {summary.renderer_terrain_root}")
    print(f"Renderer sample LOD:                  {summary.renderer_sample_lod}")
    print(f"Source raster:                        {summary.source_raster_path}")
    print(f"Center renderer ground MSL:           {summary.center_renderer_ground_msl_m:.3f} m" if summary.center_renderer_ground_msl_m is not None else "Center renderer ground MSL:           n/a")
    print(f"Center source raster ground MSL:      {summary.center_source_ground_msl_m:.3f} m" if summary.center_source_ground_msl_m is not None else "Center source raster ground MSL:      n/a")
    print(f"Center preprocess_runtime_delta_m:    {summary.center_preprocess_runtime_delta_m:.3f} m" if summary.center_preprocess_runtime_delta_m is not None else "Center preprocess_runtime_delta_m:    n/a")
    print("Grid stats:")
    print(f"  preprocess_runtime_delta_m mean:    {summary.preprocess_runtime_delta_m_mean:.3f}")
    print(f"  preprocess_runtime_delta_m min:     {summary.preprocess_runtime_delta_m_min:.3f}")
    print(f"  preprocess_runtime_delta_m max:     {summary.preprocess_runtime_delta_m_max:.3f}")
    print(f"  preprocess_runtime_delta_m rms:     {summary.preprocess_runtime_delta_m_rms:.3f}")
    print(f"  preprocess_runtime_delta_abs p50:   {summary.preprocess_runtime_delta_abs_p50_m:.3f}")
    print(f"  preprocess_runtime_delta_abs p95:   {summary.preprocess_runtime_delta_abs_p95_m:.3f}")
    print(f"  preprocess_runtime_delta_abs max:   {summary.preprocess_runtime_delta_abs_max_m:.3f}")

    if args.json_out:
        args.json_out.write_text(json.dumps(asdict(summary), indent=2))
        print(f"JSON summary:                         {args.json_out}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
