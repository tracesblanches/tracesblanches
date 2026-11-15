#!/usr/bin/env bash
# Fails if a versioned data file (bench inputs, fuzz seeds, regressions and corpus) holds
# data of a source that may not be published: Biodiv'sports zones (licence not verified,
# F5 #11) or the CLPA avalanche map (fused into the exposure layer). Gzipped files are
# inflated first. Code and documentation may name these sources: only data is checked.
#
#   .github/scripts/check-unpublishable-data.sh        (CI job `workflows`)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
pattern='biodiv|clpa'
found=0
while IFS= read -r -d '' f; do
  case $f in
    *.gz) content() { gzip -dc -- "$f" 2>/dev/null || true; } ;;
    *) content() { cat -- "$f"; } ;;
  esac
  n=$(content | grep -aciE "$pattern" || true)
  if [ "$n" != 0 ]; then
    echo "::error file=$f::$n line(s) matching /$pattern/i"
    content | grep -aoiE ".{0,40}($pattern).{0,40}" | head -3
    found=1
  fi
done < <(git ls-files -z -- benches/data fuzz/seeds fuzz/regressions fuzz/corpus)
# The bench exposure must be the model alone (bench_data.meta.json states its source).
if ! grep -q 'exposure_model_5m.tif' benches/data/bench_data.meta.json; then
  echo "::error file=benches/data/bench_data.meta.json::the off-piste exposure is not the model alone"
  found=1
fi
if [ "$found" = 0 ]; then
  echo "no Biodiv'sports or CLPA data in $(git ls-files -- benches/data fuzz/seeds fuzz/regressions fuzz/corpus | wc -l) versioned data files"
fi
exit "$found"
