#!/usr/bin/env bash
# Regenerates all assets: timings and level → Blender models → sound effects.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run -q --bin export_timings > tools/blender/timings.json
for s in player boss weapons hound puppet trial_bosses boss_icons class_icons arena; do
    echo "== $s"
    blender -b --factory-startup -P "tools/blender/$s.py" 2>&1 | grep -E "\[assets\]|Error|Traceback" || true
done
python3 tools/sfx.py
python3 tools/pixel_font.py
