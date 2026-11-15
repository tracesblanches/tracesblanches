#!/usr/bin/env bash
# Instruction-count regression check (Q5, #105), used by CI and on the workstation.
#
#   benches/compare.sh <base revision> [limit]
#
# Runs the gungraun benchmarks of the dominating cases (skimap-core `dominant`,
# skimap-avalanche `exposure`) on the base revision (exported with `git archive` into a
# temporary directory) and on the working tree, on the same machine, and fails with exit
# code 3 when an instruction count grew by more than <limit> (default 5%). Instruction
# counts are deterministic for a given binary (exactly equal from run to run), but differ
# between machines (glibc, Valgrind version), hence a comparison on the same runner rather
# than stored numbers.
#
# When the benchmarks or their inputs changed between the two revisions (benches/data,
# crates/*/benches/), the counts are not comparable: the working tree is measured and the
# comparison is skipped. Needs valgrind and gungraun-runner 0.20.0 in PATH.
set -euo pipefail

base=${1:?usage: benches/compare.sh <base revision> [limit]}
limit=${2:-5%}
root=$(git rev-parse --show-toplevel)
cd "$root"
benches=(-p skimap-core --bench dominant -p skimap-avalanche --bench exposure)
export GUNGRAUN_HOME="$root/target/gungraun-compare"
rm -rf "$GUNGRAUN_HOME"

base_sha=$(git rev-parse --verify "$base^{commit}")
echo "base $base ($base_sha), head $(git rev-parse HEAD) + working tree, limit ir=$limit"

if ! git diff --quiet "$base_sha" -- benches/data crates/core/benches crates/avalanche/benches; then
  echo "benchmarks or their inputs changed since $base_sha: measuring the head only"
  cargo bench --locked "${benches[@]}"
  exit 0
fi

wt=$(mktemp -d)
trap 'rm -rf "$wt"' EXIT
git archive "$base_sha" | tar -x -C "$wt"
# Fresh modification times (the archive keeps the commit time, which can be older than an
# earlier build) and a target directory of its own: the benchmark binaries embed the path
# of their sources (CARGO_MANIFEST_DIR, to find benches/data), so a binary built from the
# base must never be taken for the head's, or the other way round.
find "$wt" -type f -exec touch {} +
echo "== base"
if ! (cd "$wt" && CARGO_TARGET_DIR="$root/target/bench-base" cargo bench --locked "${benches[@]}" -- --save-baseline=base); then
  echo "the base revision has no runnable benchmarks: measuring the head only"
  cargo bench --locked "${benches[@]}"
  exit 0
fi
echo "== head"
cargo bench --locked "${benches[@]}" -- --baseline=base --callgrind-limits="ir=$limit"
