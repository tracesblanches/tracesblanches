#!/usr/bin/env bash
# Short fuzzing for CI (Q3 #103, Q6 #106) and the workstation.
#
#   fuzz/ci.sh [seconds per target] [targets in parallel]
#
# Needs a nightly toolchain ($FUZZ_TOOLCHAIN, default nightly) and cargo-fuzz. For every
# target: replays the versioned inputs (seeds/, regressions/, corpus/) once under the
# address sanitizer and the libFuzzer limits, then fuzzes for the given time, new inputs
# going to work/<target> (kept between CI runs by the cache). Fails if any target crashed,
# panicked, leaked out of the memory limit or timed out; the crashing input is in
# artifacts/<target>/.
set -euo pipefail
cd "$(dirname "$0")"
secs=${1:-60}
par=${2:-$(nproc)}
tc=${FUZZ_TOOLCHAIN:-nightly}
mkdir -p logs work artifacts
cargo "+$tc" fuzz build
mapfile -t targets < <(cargo "+$tc" fuzz list)
bin=target/x86_64-unknown-linux-gnu/release

max_len() {
  case $1 in
    pack | pack_update | pmtiles_vendor | tile_archive | geotiff | offpiste_grid) echo 262144 ;;
    *) echo 65536 ;;
  esac
}

run_one() {
  local t=$1 secs=$2 bin=$3 ml
  ml=$(max_len "$t")
  mkdir -p "work/$t" "artifacts/$t"
  local dirs=()
  for d in seeds regressions corpus; do
    [ -d "$d/$t" ] && dirs+=("$d/$t")
  done
  local limits=(-rss_limit_mb=2048 -malloc_limit_mb=2048 -timeout=20 "-max_len=$ml")
  if ! "$bin/$t" "${limits[@]}" -artifact_prefix="artifacts/$t/" -runs=0 "${dirs[@]}" \
    > "logs/$t.replay.log" 2>&1; then
    echo "FAIL $t: replay of the versioned inputs"
    tail -40 "logs/$t.replay.log"
    return 1
  fi
  if ! "$bin/$t" "${limits[@]}" -artifact_prefix="artifacts/$t/" -max_total_time="$secs" \
    -print_final_stats=1 "work/$t" "${dirs[@]}" > "logs/$t.fuzz.log" 2>&1; then
    echo "FAIL $t: crash while fuzzing"
    tail -60 "logs/$t.fuzz.log"
    return 1
  fi
  echo "ok   $t: $(grep -E '^stat::number_of_executed_units' "logs/$t.fuzz.log" | awk '{print $2}') executions in ${secs} s, $(find "work/$t" -type f | wc -l) inputs in work/$t"
}
export -f run_one max_len

printf '%s\n' "${targets[@]}" | xargs -P "$par" -I{} bash -c 'run_one "$@"' _ {} "$secs" "$bin"
