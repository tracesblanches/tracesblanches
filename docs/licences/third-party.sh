#!/usr/bin/env bash
# Writes THIRD-PARTY.md at the root of the repository (F2, #2): the licence of every
# third-party component, generated from the lock files, never typed by hand.
#
#   docs/licences/third-party.sh            # rewrites THIRD-PARTY.md
#   docs/licences/third-party.sh --check    # exits 1 if THIRD-PARTY.md is out of date
#
# Sources, all local (no network):
# - Rust: `cargo metadata --locked` (licence expression of each crate of Cargo.lock, every
#   feature, Linux and Android targets) and `cargo tree -p skimap-ffi` for the crates
#   linked into the Android library (normal edges, Android target, no proc-macro);
# - Android: `./gradlew :app:dependencies --configuration releaseRuntimeClasspath
#   --offline` (resolved versions of the APK), licences read from the POM files of the
#   Gradle cache (`~/.gradle/caches/modules-2`), following `<parent>` when a POM has none.
#   Gradle keeps no POM for a library that publishes Gradle module metadata (all of
#   AndroidX): its POM is then fetched once from Google Maven, else Maven Central, into
#   `~/.cache/skimap/poms` (public metadata only, nothing about the user is sent);
# - components that are not a dependency (ported code, vendored crate, models): the fixed
#   table below, from docs/licences/f2-licence-code.md.
# Needs: cargo, jq, xmllint, a JDK and the Android SDK (`ANDROID_HOME` or
# android/local.properties) with the Gradle cache already filled by a build.
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$root"
check=0
[[ ${1:-} == --check ]] && check=1
out=$(mktemp)
tmp=$(mktemp -d)
trap 'rm -rf "$out" "$tmp"' EXIT

# ---------- Rust ----------
cargo metadata --format-version 1 --locked --all-features \
  --filter-platform x86_64-unknown-linux-gnu \
  --filter-platform aarch64-linux-android \
  --filter-platform x86_64-linux-android >"$tmp/meta.json"
cargo tree --locked -p skimap-ffi -e normal,no-proc-macro --target aarch64-linux-android \
  --prefix none -f '{p}' | awk '{ sub(/^v/, "", $2); print $1 "\t" $2 }' | sort -u >"$tmp/app.txt"
jq -r '.packages[] | select(.source != null)
  | [.name, .version, (.license // "?"), (.repository // "")] | @tsv' "$tmp/meta.json" \
  | sort -f -t $'\t' -k1,1 -k2,2V >"$tmp/crates.tsv"
n_crates=$(wc -l <"$tmp/crates.tsv")
n_app=$(awk -F'\t' 'NR==FNR { app[$1 " " $2] = 1; next } ($1 " " $2) in app' \
  "$tmp/app.txt" "$tmp/crates.tsv" | wc -l)

# ---------- Android ----------
(cd android && ./gradlew -q --offline :app:dependencies \
  --configuration releaseRuntimeClasspath) >"$tmp/gradle.txt"
# "+--- g:a:v", "\--- g:a:v1 -> v2 (*)", "+--- g:a -> v"; constraints "(c)" are not
# dependencies, project modules are ours.
grep -E -- '--- ' "$tmp/gradle.txt" | sed -E 's/^.*--- //' \
  | grep -v -E '^project |\(c\)$' | sed -E 's/ \((\*|n)\)$//' \
  | awk '{
      # Requested version (plain or rich, "{prefer 2.1.0}"), then the resolved one.
      split($1, c, ":"); v = c[3]
      n = split($0, r, " -> ")
      if (n > 1) { split(r[n], w, " "); v = w[1] }
      print c[1] ":" c[2] ":" v
    }' | sort -u >"$tmp/aar.txt"
cache=${GRADLE_USER_HOME:-$HOME/.gradle}/caches/modules-2/files-2.1
poms=${XDG_CACHE_HOME:-$HOME/.cache}/skimap/poms
mkdir -p "$poms"
pom() { # group artifact version -> path of its POM (Gradle cache, else fetched once)
  local f
  f=$(find "$cache/$1/$2/$3" -name "$2-$3.pom" 2>/dev/null | head -n 1)
  [[ -n $f ]] && { echo "$f"; return; }
  f=$poms/$1/$2-$3.pom
  if [[ ! -s $f ]]; then
    mkdir -p "$poms/$1"
    local path=${1//.//}/$2/$3/$2-$3.pom repo
    for repo in https://dl.google.com/dl/android/maven2 https://repo.maven.apache.org/maven2; do
      curl -sSfL --retry 2 "$repo/$path" -o "$f.part" 2>/dev/null && mv "$f.part" "$f" && break
    done
    rm -f "$f.part"
  fi
  [[ -s $f ]] && echo "$f"
}
xp() { xmllint --xpath "$2" "$1" 2>/dev/null || true; }
licences() { # group artifact version -> "licence; licence"
  local g=$1 a=$2 v=$3 p depth=0
  while ((depth < 6)); do
    p=$(pom "$g" "$a" "$v")
    [[ -z $p ]] && { echo "?"; return; }
    local names
    names=$(xp "$p" "//*[local-name()='project']/*[local-name()='licenses']/*[local-name()='license']/*[local-name()='name']/text()" \
      | sed -E 's/^[[:space:]]+|[[:space:]]+$//g' | grep -v '^$' | paste -sd ';' | sed 's/;/ ; /g')
    if [[ -n $names ]]; then
      echo "$names"
      return
    fi
    local pg pa pv
    pg=$(xp "$p" "string(//*[local-name()='project']/*[local-name()='parent']/*[local-name()='groupId'])")
    pa=$(xp "$p" "string(//*[local-name()='project']/*[local-name()='parent']/*[local-name()='artifactId'])")
    pv=$(xp "$p" "string(//*[local-name()='project']/*[local-name()='parent']/*[local-name()='version'])")
    [[ -z $pa ]] && { echo "?"; return; }
    g=$pg a=$pa v=$pv depth=$((depth + 1))
  done
  echo "?"
}
while IFS=: read -r g a v; do
  printf '%s\t%s\t%s\n' "$g:$a" "$v" "$(licences "$g" "$a" "$v")"
done <"$tmp/aar.txt" >"$tmp/aar.tsv"
n_aar=$(wc -l <"$tmp/aar.tsv")
n_unknown=$(awk -F'\t' '$3 == "?"' "$tmp/aar.tsv" | wc -l)
n_unknown_rust=$(awk -F'\t' '$3 == "?"' "$tmp/crates.tsv" | wc -l)

# ---------- Document ----------
{
  cat <<'EOF'
# Composants tiers et leurs licences

Fichier généré par `docs/licences/third-party.sh` à partir de `Cargo.lock` et de la
résolution Gradle de l'app ; ne pas l'éditer à la main. `docs/licences/third-party.sh
--check` échoue s'il n'est plus à jour.

La licence du code de skimap est décrite dans le [README](README.md#licence) ; la licence
des données produites (packs, couches), par source, dans
[docs/licences-donnees.md](docs/licences-donnees.md).

## Code et modèles repris hors dépendances

| Composant | Licence | Ce que le dépôt en fait |
|---|---|---|
| com4FlowPy (AvaFrame), commit `d0451ce` | EUPL-1.2 | `crates/avalanche/src/exposure/flowpy.rs` en est un portage Rust ; la crate `skimap-avalanche` reste sous EUPL-1.2 ([LICENSES/EUPL-1.2.txt](LICENSES/EUPL-1.2.txt)) et n'est liée que par le pipeline, jamais par l'app |
| AutoATES v3.0, « Copyright (c) 2024 AutoATES » | MIT | zones de départ, classes et paramètres portés dans `crates/avalanche` ; l'oracle Python est hors du chemin de calcul |
| pmtiles 0.28.1 (`vendor/pmtiles`) | MIT OR Apache-2.0 | copie corrigée, utilisée sous MIT ; textes dans `vendor/pmtiles/LICENSE-MIT` et `LICENSE-APACHE`, modification décrite dans `vendor/pmtiles/VENDOR.md` |
| Protomaps basemaps | BSD-3-Clause | style de départ de `crates/pack/assets/protomaps/style-light-fr.json` |
| snowtools (CNRM) | CeCILL-C v1 | aucun fichier dans le dépôt ; contours des massifs lus par le pipeline, cités dans la couche qui en dérive |
| Pocket TTS (Kyutai), modèle | CC-BY-4.0 et conditions d'usage du modèle | génère les clips de voix sur le poste ; non livré dans l'app |
| pockettts.cpp, ggml | MIT | outil du poste pour la voix ; non livré dans l'app |

## Crates Rust
EOF
  echo
  echo "$n_crates crates tierces dans \`Cargo.lock\` (toutes fonctions, cibles Linux et Android), dont $n_app liées dans la bibliothèque de l'app (\`skimap-ffi\`, colonne « App ») ; licence inconnue : $n_unknown_rust. Une licence « A OR B » laisse le choix ; skimap n'en retire aucune option."
  echo
  echo '| Crate | Version | Licence | App | Dépôt |'
  echo '|---|---|---|---|---|'
  awk -F'\t' 'NR==FNR { app[$1 " " $2] = 1; next }
    { printf "| %s | %s | %s | %s | %s |\n", $1, $2, $3, (($1 " " $2) in app) ? "oui" : "", $4 }' \
    "$tmp/app.txt" "$tmp/crates.tsv"
  echo
  echo '## Bibliothèques Android'
  echo
  echo "$n_aar bibliothèques dans le classpath d'exécution de la version release de l'app (\`:app\`, \`releaseRuntimeClasspath\`), licence lue dans leur POM (plusieurs licences séparées par « ; » : toutes celles que le POM déclare) ; licence absente du POM : $n_unknown."
  echo
  echo '| Bibliothèque | Version | Licence (POM) |'
  echo '|---|---|---|'
  awk -F'\t' '{ printf "| %s | %s | %s |\n", $1, $2, $3 }' "$tmp/aar.tsv"
} >"$out"

if ((check)); then
  if ! diff -u THIRD-PARTY.md "$out"; then
    echo "THIRD-PARTY.md is out of date: run docs/licences/third-party.sh" >&2
    exit 1
  fi
  echo "THIRD-PARTY.md is up to date ($n_crates crates, $n_aar Android libraries)"
else
  cp "$out" THIRD-PARTY.md
  echo "THIRD-PARTY.md: $n_crates crates ($n_app in the app), $n_aar Android libraries, unknown licences: Rust $n_unknown_rust, Android $n_unknown"
fi
