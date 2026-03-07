#!/usr/bin/env python3
"""
Run a repeatable physical-truth matrix across representative local regions.

For each case this reports:
  - renderer vs small_world/HGT residuals
  - source raster vs small_world/HGT dataset-floor residuals
  - renderer vs source raster residuals

The last term is the renderer residual above the source-vs-small_world floor.
"""

from __future__ import annotations

import argparse
import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Dict, Iterable, List, Optional

import compare_small_world_ground as ground_compare
from compare_renderer_to_source_raster import sample_source_raster_wgs84


@dataclass(frozen=True)
class TruthCase:
    name: str
    lat_deg: float
    lon_deg: float
    note: str


@dataclass(frozen=True)
class TruthSuite:
    name: str
    note: str
    terrain_root: Path
    source_raster: Path
    cases: List[TruthCase]


@dataclass
class TruthCaseSummary:
    suite_name: str
    suite_note: str
    name: str
    note: str
    lat_deg: float
    lon_deg: float
    point_count: int
    compared_count: int
    renderer_sample_lod: int
    terrain_root: str
    source_raster: str
    center_renderer_ground_msl_m: Optional[float]
    center_source_ground_msl_m: Optional[float]
    center_small_world_ground_msl_m: Optional[float]
    center_renderer_vs_source_m: Optional[float]
    center_renderer_vs_small_world_m: Optional[float]
    center_source_vs_small_world_m: Optional[float]
    renderer_vs_source_abs_p95_m: float
    renderer_vs_small_world_abs_p95_m: float
    source_vs_small_world_abs_p95_m: float
    renderer_vs_source_rms_m: float
    renderer_vs_small_world_rms_m: float
    source_vs_small_world_rms_m: float


def default_suite_map(project_root: Path) -> Dict[str, TruthSuite]:
    return {
        "earth_global": TruthSuite(
            name="earth_global",
            note="global base earth height asset",
            terrain_root=project_root / "assets" / "terrains" / "earth",
            source_raster=project_root / "source_data" / "gebco_earth_small.tif",
            cases=[
                TruthCase("alps_peak", 46.55, 10.60, "steep relief, central Alps"),
                TruthCase("alps_west_slope", 46.45, 10.20, "steep relief, west slope"),
                TruthCase("alps_east_slope", 46.70, 10.85, "steep relief, east slope"),
                TruthCase("florida_keys", 24.70, -81.30, "low relief, coastal keys"),
                TruthCase("florida_lower_keys", 24.55, -81.75, "low relief, lower keys"),
                TruthCase(
                    "florida_north_tile",
                    24.95,
                    -81.55,
                    "low relief, north edge of covered tile",
                ),
            ],
        ),
        "swiss_overlay": TruthSuite(
            name="swiss_overlay",
            note="regional Swiss overlay with local HGT overlap on the eastern strip",
            terrain_root=project_root / "assets" / "terrains" / "swiss_highres",
            source_raster=project_root / "source_data" / "swiss.tif",
            cases=[
                TruthCase(
                    "swiss_border_south",
                    46.45,
                    10.20,
                    "overlay/HGT overlap, south-east border strip",
                ),
                TruthCase(
                    "swiss_border_high_relief",
                    46.70,
                    10.20,
                    "overlay/HGT overlap, high relief east strip",
                ),
                TruthCase(
                    "swiss_border_north",
                    46.70,
                    10.40,
                    "overlay/HGT overlap, north-east border strip",
                ),
            ],
        ),
    }


def percentile_abs(values: List[float], fraction: float) -> float:
    return ground_compare.percentile([abs(v) for v in values], fraction)


def run_case(
    suite: TruthSuite,
    case: TruthCase,
    hgt_sampler: ground_compare.HgtSampler,
    renderer_sample_lod: int,
    half_size_m: float,
    step_m: float,
) -> TruthCaseSummary:
    renderer_vs_source: List[float] = []
    renderer_vs_small_world: List[float] = []
    source_vs_small_world: List[float] = []

    point_count = 0
    compared_count = 0

    center_renderer = ground_compare.sample_renderer_ground_msl(
        suite.terrain_root, case.lat_deg, case.lon_deg, renderer_sample_lod
    )
    center_source = sample_source_raster_wgs84(suite.source_raster, case.lat_deg, case.lon_deg)
    center_small_world = hgt_sampler.sample_bilinear(case.lat_deg, case.lon_deg)

    north_m = -half_size_m
    while north_m <= half_size_m + 1e-6:
        east_m = -half_size_m
        while east_m <= half_size_m + 1e-6:
            point_count += 1
            lat_deg, lon_deg = ground_compare.offset_lat_lon(
                case.lat_deg, case.lon_deg, north_m, east_m
            )
            renderer_ground = ground_compare.sample_renderer_ground_msl(
                suite.terrain_root, lat_deg, lon_deg, renderer_sample_lod
            )
            source_ground = sample_source_raster_wgs84(suite.source_raster, lat_deg, lon_deg)
            small_world_ground = hgt_sampler.sample_bilinear(lat_deg, lon_deg)

            if (
                renderer_ground is None
                or source_ground is None
                or small_world_ground is None
            ):
                east_m += step_m
                continue

            renderer_vs_source.append(renderer_ground - source_ground)
            renderer_vs_small_world.append(renderer_ground - small_world_ground)
            source_vs_small_world.append(source_ground - small_world_ground)
            compared_count += 1

            east_m += step_m
        north_m += step_m

    if not renderer_vs_source:
        raise RuntimeError(f"No comparable samples found for case {case.name}")

    return TruthCaseSummary(
        suite_name=suite.name,
        suite_note=suite.note,
        name=case.name,
        note=case.note,
        lat_deg=case.lat_deg,
        lon_deg=case.lon_deg,
        point_count=point_count,
        compared_count=compared_count,
        renderer_sample_lod=renderer_sample_lod,
        terrain_root=str(suite.terrain_root.resolve()),
        source_raster=str(suite.source_raster.resolve()),
        center_renderer_ground_msl_m=center_renderer,
        center_source_ground_msl_m=center_source,
        center_small_world_ground_msl_m=center_small_world,
        center_renderer_vs_source_m=(
            None
            if center_renderer is None or center_source is None
            else center_renderer - center_source
        ),
        center_renderer_vs_small_world_m=(
            None
            if center_renderer is None or center_small_world is None
            else center_renderer - center_small_world
        ),
        center_source_vs_small_world_m=(
            None
            if center_source is None or center_small_world is None
            else center_source - center_small_world
        ),
        renderer_vs_source_abs_p95_m=percentile_abs(renderer_vs_source, 0.95),
        renderer_vs_small_world_abs_p95_m=percentile_abs(renderer_vs_small_world, 0.95),
        source_vs_small_world_abs_p95_m=percentile_abs(source_vs_small_world, 0.95),
        renderer_vs_source_rms_m=ground_compare.rms(renderer_vs_source),
        renderer_vs_small_world_rms_m=ground_compare.rms(renderer_vs_small_world),
        source_vs_small_world_rms_m=ground_compare.rms(source_vs_small_world),
    )


def print_case_summary(summary: TruthCaseSummary) -> None:
    print(
        f"{summary.suite_name:14} "
        f"{summary.name:24} "
        f"lat={summary.lat_deg:7.3f} lon={summary.lon_deg:8.3f}  "
        f"src_floor_center={summary.center_source_vs_small_world_m:8.3f}  "
        f"renderer_center={summary.center_renderer_vs_small_world_m:8.3f}  "
        f"renderer_above_floor={summary.center_renderer_vs_source_m:8.3f}  "
        f"p95_floor={summary.source_vs_small_world_abs_p95_m:8.3f}  "
        f"p95_renderer={summary.renderer_vs_small_world_abs_p95_m:8.3f}  "
        f"p95_above_floor={summary.renderer_vs_source_abs_p95_m:8.3f}"
    )


def resolve_suite_names(
    suite_map: Dict[str, TruthSuite], selected_names: Optional[Iterable[str]]
) -> List[str]:
    if not selected_names:
        return ["earth_global"]

    cleaned = [name.strip() for name in selected_names if name.strip()]
    if not cleaned:
        return ["earth_global"]
    if any(name == "all" for name in cleaned):
        return list(suite_map.keys())

    unknown = [name for name in cleaned if name not in suite_map]
    if unknown:
        raise RuntimeError(f"Unknown truth suite(s): {', '.join(unknown)}")
    return cleaned


def resolve_cases(suite: TruthSuite, selected_names: Optional[Iterable[str]]) -> List[TruthCase]:
    if not selected_names:
        return list(suite.cases)

    selected = {name.strip() for name in selected_names if name.strip()}
    return [case for case in suite.cases if case.name in selected]


def main() -> int:
    project_root = Path(__file__).resolve().parents[1]
    suite_map = default_suite_map(project_root)

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--suite",
        type=str,
        default="earth_global",
        help=(
            "Comma-separated suite names. Available: "
            + ", ".join(list(suite_map.keys()) + ["all"])
        ),
    )
    parser.add_argument("--terrain-root", type=Path, default=None)
    parser.add_argument("--source-raster", type=Path, default=None)
    parser.add_argument("--hgt-root", type=Path, default=None)
    parser.add_argument("--renderer-sample-lod", type=int, default=None)
    parser.add_argument("--half-size-m", type=float, default=1000.0)
    parser.add_argument("--step-m", type=float, default=250.0)
    parser.add_argument(
        "--cases",
        type=str,
        default="",
        help="Comma-separated subset of case names",
    )
    parser.add_argument("--json-out", type=Path, default=None)
    args = parser.parse_args()

    if args.hgt_root is not None:
        hgt_root = args.hgt_root
    else:
        sibling_default = project_root.parent / "planetary_test" / "data" / "srtm"
        if sibling_default.is_dir():
            hgt_root = sibling_default
        else:
            hgt_root = ground_compare.resolve_hgt_root(project_root, None)
    hgt_sampler = ground_compare.HgtSampler(hgt_root)
    suite_names = resolve_suite_names(suite_map, args.suite.split(",") if args.suite else None)

    if (args.terrain_root is not None or args.source_raster is not None) and len(suite_names) > 1:
        raise RuntimeError("--terrain-root/--source-raster overrides require a single selected suite")

    summaries: List[TruthCaseSummary] = []
    selected_case_names = args.cases.split(",") if args.cases else None

    for suite_name in suite_names:
        suite = suite_map[suite_name]
        if args.terrain_root is not None:
            suite = TruthSuite(
                name=suite.name,
                note=suite.note,
                terrain_root=args.terrain_root,
                source_raster=args.source_raster or suite.source_raster,
                cases=suite.cases,
            )
        elif args.source_raster is not None:
            suite = TruthSuite(
                name=suite.name,
                note=suite.note,
                terrain_root=suite.terrain_root,
                source_raster=args.source_raster,
                cases=suite.cases,
            )

        cases = resolve_cases(suite, selected_case_names)
        if not cases:
            continue

        renderer_sample_lod = ground_compare.resolve_renderer_sample_lod(
            suite.terrain_root, args.renderer_sample_lod
        )

        for case in cases:
            summaries.append(
                run_case(
                    suite,
                    case,
                    hgt_sampler,
                    renderer_sample_lod,
                    args.half_size_m,
                    args.step_m,
                )
            )

    if not summaries:
        raise RuntimeError("No truth-matrix cases selected")

    print("Physical truth matrix")
    print(
        "suite          case                     "
        "location                       "
        "src_floor_center  renderer_center  renderer_above_floor  "
        "p95_floor   p95_renderer  p95_above_floor"
    )
    for summary in summaries:
        print_case_summary(summary)

    if args.json_out is not None:
        args.json_out.parent.mkdir(parents=True, exist_ok=True)
        args.json_out.write_text(
            json.dumps([asdict(summary) for summary in summaries], indent=2) + "\n",
            encoding="utf-8",
        )
        print(f"JSON summary: {args.json_out}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
