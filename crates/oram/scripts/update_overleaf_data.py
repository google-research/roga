#!/usr/bin/env python3
# Copyright 2026 Google LLC
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     https://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Converts target/query_scaling.csv to Overleaf CSVs without pandas."""

import csv
import math
import os


def convert():
  raw_csv = os.environ.get("RAW_CSV_PATH", "target/query_scaling.csv")
  paper_dir = os.environ.get("PAPER_DATA_DIR", "data")
  os.makedirs(paper_dir, exist_ok=True)

  rows = []
  with open(raw_csv, "r") as f:
    reader = csv.DictReader(f)
    for r in reader:
      rows.append(r)

  # Group by distribution
  dists = set(r["distribution"] for r in rows)

  for dist in dists:
    dist_name = "uniform" if dist == "uniform" else "zipf"
    dist_rows = [r for r in rows if r["distribution"] == dist]

    # Group by num_ops
    ops_map = {}
    for r in dist_rows:
      ops = int(r["num_ops"])
      backend = r["backend"]
      us = float(r["us_per_op"])
      cap = int(r["final_capacity"])
      if ops not in ops_map:
        ops_map[ops] = {}
      ops_map[ops][backend] = (us, cap)

    out_rows = []
    for ops in sorted(ops_map.keys()):
      fixed_us = ops_map[ops].get(
          "OSAM fixed", ops_map[ops].get("oram-fixed", (0.0, 0))
      )[0]
      oracle_fixed_us = ops_map[ops].get("OSAM oracle fixed", (fixed_us, 0))[0]
      resizing_us, resizing_cap = ops_map[ops].get(
          "OSAM resizing", ops_map[ops].get("oram-resizing", (0.0, 0))
      )
      cap_log2 = int(math.log2(resizing_cap)) if resizing_cap > 0 else 0
      out_rows.append({
          "queries": ops,
          "fixed_us_per_op": f"{fixed_us:.6f}",
          "oracle_fixed_us_per_op": f"{oracle_fixed_us:.6f}",
          "resizing_us_per_op": f"{resizing_us:.6f}",
          "resizing_capacity_log2": cap_log2,
      })

    out_file = os.path.join(paper_dir, f"oram_resize_{dist_name}.csv")
    with open(out_file, "w", newline="") as f:
      writer = csv.DictWriter(
          f,
          fieldnames=[
              "queries",
              "fixed_us_per_op",
              "oracle_fixed_us_per_op",
              "resizing_us_per_op",
              "resizing_capacity_log2",
          ],
      )
      writer.writeheader()
      writer.writerows(out_rows)
    print(f"Successfully updated Overleaf data CSV: {out_file}")


if __name__ == "__main__":
  convert()
