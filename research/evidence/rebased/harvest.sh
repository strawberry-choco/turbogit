#!/bin/sh
# Start the Rebased parity research. This is the entry point.
#
#   1. re-harvest the installed Rebased bundle (actions, strings, keymap)
#   2. verify + fetch every documentation page named in surfaces.tsv, convert
#      each to plain text under docs-text/
#   3. emit one spec skeleton per surface under parity/, preloaded with the real
#      action inventory and the list of sources that actually resolved
#   4. write HARVEST-REPORT.md: what we have, and which docs 404'd
#
# Usage:  research/evidence/rebased/harvest.sh [path-to-Rebased.app]
set -eu

APP="${1:-/Applications/Rebased.app}"
HERE="$(cd "$(dirname "$0")" && pwd)"
PY="${PYTHON:-/Users/kye/.workbuddy/binaries/python/versions/3.13.12/bin/python3}"
[ -x "$PY" ] || PY="$(command -v python3)"

echo "== bundle =="
"$HERE/extract.sh" "$APP"

echo
echo "== documentation + spec skeletons =="
"$PY" "$HERE/harvest.py" "$HERE"

echo
echo "== next =="
echo "Docs written to $HERE/docs-text/, skeletons to $HERE/parity/."
echo "See HARVEST-REPORT.md for gaps, then run the agent pass in DISTILL.md:"
echo "those skeletons become the full parity document (docs/spec/rebased-parity.md)."
