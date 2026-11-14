"""Reference run of AutoATES v3.0 (ISSW 2026 snapshot) on one skimap site: oracle only.

Python never runs in the skimap pipeline. This script drives the upstream reference
implementation, unmodified, on the inputs written by
`cargo run --release -p skimap-avalanche --example exposure_inputs -- <site>`, so that the
Rust implementation can be compared cell by cell. It runs in an environment outside the
repository (see crates/avalanche/docs/exposure.md, section « Oracle »).

    python -I autoates_oracle.py <dem.tif> <canopy.tif> <work_dir> [--procs N]

Model parameters: the ISSW defaults of autoates/comAutoATES/autoATESCfg.ini, untouched.
Only execution settings differ from the defaults, and none changes a result:
  * [FlowPy] tile_size is set above the domain size: one tile, no overlap recomputation
    (com4FlowPy tiles a 15 km domain into 2 x 2 tiles overlapping by 12 km otherwise);
  * com4FlowPy cpuCount and maxChunks are set to --procs: the default 500 chunks would
    each return 12 full-size float32 arrays (275 GB at 5 m over Cervieres);
  * no AOI clipping (the whole DEM is processed), no colourised outputs.
"""
import argparse
import json
import os
import resource
import sys
import time
from pathlib import Path

AUTOATES = Path(os.environ["AUTOATES_ROOT"])
AVAFRAME = Path(os.environ["AVAFRAME_ROOT"])
sys.path[:0] = [str(AUTOATES), str(AVAFRAME)]

from autoates.comAutoATES import comAutoATES  # noqa: E402
from autoates.comAutoATES.runout import runoutCore  # noqa: E402
from avaframe.com4FlowPy import com4FlowPy  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument("dem")
ap.add_argument("canopy")
ap.add_argument("work_dir")
ap.add_argument("--procs", type=int, default=16)
a = ap.parse_args()

cfg = comAutoATES.load_config(AUTOATES / "autoates/comAutoATES/autoATESCfg.ini")
cfg["General"]["working_dir"] = str(Path(a.work_dir).resolve())
cfg["General"]["aoi_path"] = ""
cfg["Input"]["dem_path"] = str(Path(a.dem).resolve())
cfg["Input"]["forest_canopy_path"] = str(Path(a.canopy).resolve())
cfg["Processing"]["run_colorize"] = "False"
cfg["FlowPy"]["tile_size"] = "1000000"

_main = com4FlowPy.com4FlowPyMain


def _main_with_procs(cfg_path, cfg_setup):
    cfg_setup["cpuCount"] = str(a.procs)
    cfg_setup["maxChunks"] = str(a.procs)
    return _main(cfg_path, cfg_setup)


runoutCore.com4FlowPy.com4FlowPyMain = _main_with_procs

t0 = time.perf_counter()
res = comAutoATES.runAutoATES(config=cfg)
wall = time.perf_counter() - t0
ru_self = resource.getrusage(resource.RUSAGE_SELF)
ru_children = resource.getrusage(resource.RUSAGE_CHILDREN)
summary = {
    "status": res.get("status"),
    "wall_clock_s": round(wall, 1),
    "cpu_s_self": round(ru_self.ru_utime + ru_self.ru_stime, 1),
    "cpu_s_children": round(ru_children.ru_utime + ru_children.ru_stime, 1),
    "max_rss_mb_self": round(ru_self.ru_maxrss / 1024),
    "max_rss_mb_largest_child": round(ru_children.ru_maxrss / 1024),
    "procs": a.procs,
    "dem": a.dem,
    "canopy": a.canopy,
}
Path(a.work_dir, "oracle_summary.json").write_text(json.dumps(summary, indent=2))
print(json.dumps(summary, indent=2))
