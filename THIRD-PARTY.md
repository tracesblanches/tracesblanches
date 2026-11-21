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

378 crates tierces dans `Cargo.lock` (toutes fonctions, cibles Linux et Android), dont 121 liées dans la bibliothèque de l'app (`skimap-ffi`, colonne « App ») ; licence inconnue : 0. Une licence « A OR B » laisse le choix ; skimap n'en retire aucune option.

| Crate | Version | Licence | App | Dépôt |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | oui | https://github.com/oyvindln/adler2 |
| aes | 0.9.4 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/block-ciphers |
| aho-corasick | 1.1.5 | Unlicense OR MIT |  | https://github.com/BurntSushi/aho-corasick |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | oui | https://github.com/zakarumych/allocator-api2 |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | oui | https://github.com/nical/android_system_properties |
| anstream | 1.0.0 | MIT OR Apache-2.0 |  | https://github.com/rust-cli/anstyle.git |
| anstyle | 1.0.14 | MIT OR Apache-2.0 |  | https://github.com/rust-cli/anstyle.git |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 |  | https://github.com/rust-cli/anstyle.git |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 |  | https://github.com/rust-cli/anstyle.git |
| anyhow | 1.0.104 | MIT OR Apache-2.0 | oui | https://github.com/dtolnay/anyhow |
| approx | 0.5.1 | Apache-2.0 | oui | https://github.com/brendanzab/approx |
| ascii | 1.1.0 | Apache-2.0 OR MIT |  | https://github.com/tomprogrammer/rust-ascii |
| askama | 0.16.1 | MIT OR Apache-2.0 |  | https://github.com/askama-rs/askama |
| askama_derive | 0.16.1 | MIT OR Apache-2.0 |  | https://github.com/askama-rs/askama |
| askama_macros | 0.16.1 | MIT OR Apache-2.0 |  | https://github.com/askama-rs/askama |
| askama_parser | 0.16.1 | MIT OR Apache-2.0 |  | https://github.com/askama-rs/askama |
| as-slice | 0.1.5 | MIT OR Apache-2.0 |  | https://github.com/japaric/as-slice |
| async-compression | 0.4.52 | MIT OR Apache-2.0 |  | https://github.com/Nullus157/async-compression |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT |  | https://github.com/smol-rs/atomic-waker |
| autocfg | 1.5.1 | Apache-2.0 OR MIT |  | https://github.com/cuviper/autocfg |
| base64 | 0.23.1 | MIT OR Apache-2.0 |  | https://github.com/marshallpierce/rust-base64 |
| basic-toml | 0.1.10 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/basic-toml |
| bincode-next | 3.1.2 | MIT or Apache-2.0 |  | https://github.com/Apich-Organization/bincode |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | oui | https://github.com/bitflags/bitflags |
| block-buffer | 0.12.1 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/utils |
| block-padding | 0.4.2 | MIT OR Apache-2.0 |  | https://github.com/RustCrypto/utils |
| bumpalo | 3.20.4 | MIT OR Apache-2.0 | oui | https://github.com/fitzgen/bumpalo |
| byteorder | 1.5.0 | Unlicense OR MIT | oui | https://github.com/BurntSushi/byteorder |
| bytes | 1.12.1 | MIT | oui | https://github.com/tokio-rs/bytes |
| bzip2 | 0.6.1 | MIT OR Apache-2.0 |  | https://github.com/trifectatechfoundation/bzip2-rs |
| camino | 1.2.6 | MIT OR Apache-2.0 | oui | https://github.com/camino-rs/camino |
| cargo_metadata | 0.23.1 | MIT | oui | https://github.com/oli-obk/cargo_metadata |
| cargo-platform | 0.3.3 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/cargo |
| cbc | 0.2.1 | MIT OR Apache-2.0 |  | https://github.com/RustCrypto/block-modes |
| cc | 1.11.2 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/cc-rs |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/cfg-if |
| chacha20 | 0.10.2 | MIT OR Apache-2.0 |  | https://github.com/RustCrypto/stream-ciphers |
| chrono | 0.4.45 | MIT OR Apache-2.0 | oui | https://github.com/chronotope/chrono |
| chunked_transfer | 1.5.0 | MIT OR Apache-2.0 |  | https://github.com/frewsxcv/rust-chunked-transfer |
| cipher | 0.5.2 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/traits |
| clap | 4.6.7 | MIT OR Apache-2.0 |  | https://github.com/clap-rs/clap |
| clap_builder | 4.6.7 | MIT OR Apache-2.0 |  | https://github.com/clap-rs/clap |
| clap_derive | 4.6.7 | MIT OR Apache-2.0 |  | https://github.com/clap-rs/clap |
| clap_lex | 1.1.1 | MIT OR Apache-2.0 |  | https://github.com/clap-rs/clap |
| cmov | 0.5.4 | Apache-2.0 OR MIT | oui | https://github.com/RustCrypto/utils |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 |  | https://github.com/rust-cli/anstyle.git |
| combine | 4.6.9 | MIT |  | https://github.com/Marwes/combine |
| compression-codecs | 0.4.47 | MIT OR Apache-2.0 |  | https://github.com/Nullus157/async-compression |
| compression-core | 0.4.34 | MIT OR Apache-2.0 |  | https://github.com/Nullus157/async-compression |
| constant_time_eq | 0.6.1 | CC0-1.0 OR MIT-0 OR Apache-2.0 | oui | https://codeberg.org/cesarb/constant_time_eq |
| const-oid | 0.10.3 | Apache-2.0 OR MIT | oui | https://github.com/RustCrypto/formats |
| core_detect | 1.0.0 | MIT/Apache-2.0 |  | https://github.com/thomcc/core_detect |
| cpubits | 0.1.1 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/utils |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.5 | MIT OR Apache-2.0 | oui | https://github.com/srijs/rust-crc32fast |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | oui | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | oui | https://github.com/crossbeam-rs/crossbeam |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | oui | https://github.com/crossbeam-rs/crossbeam |
| crypto-common | 0.2.2 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/traits |
| csv | 1.4.0 | Unlicense/MIT | oui | https://github.com/BurntSushi/rust-csv |
| csv-core | 0.1.13 | Unlicense/MIT | oui | https://github.com/BurntSushi/rust-csv |
| ctr | 0.10.1 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/block-modes |
| ctutils | 0.4.8 | Apache-2.0 OR MIT | oui | https://github.com/RustCrypto/utils |
| derive_more | 2.1.1 | MIT |  | https://github.com/JelteF/derive_more |
| derive_more-impl | 2.1.1 | MIT |  | https://github.com/JelteF/derive_more |
| digest | 0.11.3 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/traits |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 |  | https://github.com/yaahc/displaydoc |
| earcut | 0.4.5 | ISC | oui | https://github.com/ciscorn/earcut-rs |
| either | 1.19.0 | MIT OR Apache-2.0 | oui | https://github.com/rayon-rs/either |
| either-or-both | 0.3.1 | Apache-2.0 OR MIT |  | https://github.com/gamma0987/either-or-both |
| encoding_rs | 0.8.44 | (Apache-2.0 OR MIT) AND BSD-3-Clause |  | https://github.com/hsivonen/encoding_rs |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | oui | https://github.com/indexmap-rs/equivalent |
| errno | 0.3.15 | MIT OR Apache-2.0 | oui | https://github.com/lambda-fairy/rust-errno |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 |  | https://github.com/sfackler/rust-fallible-iterator |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 |  | https://github.com/sfackler/fallible-streaming-iterator |
| fast_hilbert | 2.1.0 | MIT | oui | https://github.com/becheran/fast-hilbert |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | oui | https://github.com/smol-rs/fastrand |
| fax | 0.2.7 | MIT | oui | https://github.com/pdf-rs/fax |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 |  | https://github.com/image-rs/fdeflate |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/cc-rs |
| fixedbitset | 0.5.9 | MIT OR Apache-2.0 |  | https://github.com/petgraph/fixedbitset |
| flate2 | 1.1.11 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/flate2-rs |
| float_next_after | 2.0.0 | MIT | oui | https://gitlab.com/bronsonbdevost/next_afterf |
| fnv | 1.0.7 | Apache-2.0 / MIT |  | https://github.com/servo/rust-fnv |
| foldhash | 0.1.5 | Zlib |  | https://github.com/orlp/foldhash |
| foldhash | 0.2.0 | Zlib | oui | https://github.com/orlp/foldhash |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 |  | https://github.com/servo/rust-url |
| fs-err | 3.3.2 | MIT OR Apache-2.0 |  | https://github.com/andrewhickman/fs-err |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| futures-core | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| futures-io | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| futures-sink | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| futures-task | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| futures-util | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/futures-rs |
| generic-array | 0.12.4 | MIT |  | https://github.com/fizyk20/generic-array.git |
| generic-array | 0.13.3 | MIT |  | https://github.com/fizyk20/generic-array.git |
| generic-array | 0.14.9 | MIT |  | https://github.com/fizyk20/generic-array.git |
| geo | 0.33.1 | MIT OR Apache-2.0 | oui | https://github.com/georust/geo |
| geographiclib-rs | 0.2.7 | MIT | oui | https://github.com/georust/geographiclib-rs |
| geojson | 1.0.0 | MIT/Apache-2.0 | oui | https://github.com/georust/geojson |
| geo-types | 0.7.20 | MIT OR Apache-2.0 | oui | https://github.com/georust/geo |
| getrandom | 0.2.17 | MIT OR Apache-2.0 |  | https://github.com/rust-random/getrandom |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | oui | https://github.com/rust-random/getrandom |
| glob | 0.3.4 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/glob |
| goblin | 0.8.2 | MIT |  | https://github.com/m4b/goblin |
| grib | 0.20.4 | MIT OR Apache-2.0 |  | https://github.com/noritada/grib-rs |
| grib-build | 0.4.4 | MIT OR Apache-2.0 |  | https://github.com/noritada/grib-rs |
| grib-template-derive | 0.2.3 | MIT OR Apache-2.0 |  | https://github.com/noritada/grib-rs |
| grib-template-helpers | 0.2.2 | MIT OR Apache-2.0 |  | https://github.com/noritada/grib-rs |
| gungraun | 0.20.0 | Apache-2.0 OR MIT |  | https://github.com/gungraun/gungraun |
| gungraun-common | 0.1.0 | Apache-2.0 OR MIT |  | https://github.com/gungraun/gungraun |
| gungraun-macros | 0.10.0 | Apache-2.0 OR MIT |  | https://github.com/gungraun/gungraun |
| gungraun-runner | 0.20.0 | Apache-2.0 OR MIT |  | https://github.com/gungraun/gungraun |
| h2 | 0.4.20 | MIT |  | https://github.com/hyperium/h2 |
| half | 2.7.1 | MIT OR Apache-2.0 | oui | https://github.com/VoidStarKat/half-rs |
| hash32 | 0.1.1 | MIT OR Apache-2.0 |  | https://github.com/japaric/hash32 |
| hash32 | 0.2.1 | MIT OR Apache-2.0 |  | https://github.com/japaric/hash32 |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | oui | https://github.com/japaric/hash32 |
| hashbrown | 0.15.5 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/hashbrown |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/hashbrown |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/hashbrown |
| hashlink | 0.12.2 | MIT OR Apache-2.0 |  | https://github.com/djc/hashlink |
| heapless | 0.6.1 | MIT OR Apache-2.0 |  | https://github.com/japaric/heapless |
| heapless | 0.7.17 | MIT OR Apache-2.0 |  | https://github.com/japaric/heapless |
| heapless | 0.8.0 | MIT OR Apache-2.0 | oui | https://github.com/rust-embedded/heapless |
| heck | 0.5.0 | MIT OR Apache-2.0 | oui | https://github.com/withoutboats/heck |
| hex | 0.4.3 | MIT OR Apache-2.0 | oui | https://github.com/KokaKiwi/rust-hex |
| hmac | 0.13.0 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/MACs |
| home | 0.5.12 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/cargo |
| http | 1.5.0 | MIT OR Apache-2.0 |  | https://github.com/hyperium/http |
| httparse | 1.10.1 | MIT OR Apache-2.0 |  | https://github.com/seanmonstar/httparse |
| http-body | 1.1.0 | MIT |  | https://github.com/hyperium/http-body |
| http-body-util | 0.1.5 | MIT |  | https://github.com/hyperium/http-body |
| httpdate | 1.0.3 | MIT OR Apache-2.0 |  | https://github.com/pyfisch/httpdate |
| hybrid-array | 0.4.15 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/hybrid-array |
| hyper | 1.12.0 | MIT |  | https://github.com/hyperium/hyper |
| hyper-rustls | 0.27.10 | Apache-2.0 OR ISC OR MIT |  | https://github.com/rustls/hyper-rustls |
| hyper-util | 0.1.21 | MIT |  | https://github.com/hyperium/hyper-util |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | oui | https://github.com/strawlab/iana-time-zone |
| icu_collections | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_locale_core | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_normalizer | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_properties | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_properties_data | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| icu_provider | 2.4.0 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| idna | 1.1.0 | MIT OR Apache-2.0 |  | https://github.com/servo/rust-url/ |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT |  | https://github.com/hsivonen/idna_adapter |
| i_float | 1.16.0 | MIT | oui | https://github.com/iShape-Rust/iFloat |
| i_key_sort | 0.10.3 | MIT | oui | https://github.com/iShape-Rust/iKeySort |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | oui | https://github.com/indexmap-rs/indexmap |
| inout | 0.2.6 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/utils |
| integer-encoding | 4.1.0 | MIT | oui | https://github.com/dermesser/integer-encoding-rs |
| i_overlay | 4.5.2 | MIT OR Apache-2.0 | oui | https://github.com/iShape-Rust/iOverlay |
| ipnet | 2.12.2 | MIT OR Apache-2.0 |  | https://github.com/krisprice/ipnet |
| i_shape | 1.18.0 | MIT | oui | https://github.com/iShape-Rust/iShape |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 |  | https://github.com/polyfill-rs/is_terminal_polyfill |
| itoa | 1.0.19 | MIT OR Apache-2.0 | oui | https://github.com/dtolnay/itoa |
| i_tree | 0.18.0 | MIT | oui | https://github.com/iShape-Rust/iTree |
| jni | 0.22.4 | MIT OR Apache-2.0 |  | https://github.com/jni-rs/jni-rs |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 |  | https://github.com/jni-rs/jni-rs |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 |  | https://github.com/jni-rs/jni-sys |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 |  | https://github.com/jni-rs/jni-sys |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 |  | https://github.com/rust-lang-nursery/lazy-static.rs |
| libbz2-rs-sys | 0.2.6 | bzip2-1.0.6 |  | https://github.com/trifectatechfoundation/libbzip2-rs |
| libc | 0.2.191 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/libc |
| libm | 0.2.16 | MIT | oui | https://github.com/rust-lang/compiler-builtins |
| libsqlite3-sys | 0.40.0 | MIT |  | https://github.com/rusqlite/rusqlite |
| linux-raw-sys | 0.4.15 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |  | https://github.com/sunfishcode/linux-raw-sys |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | oui | https://github.com/sunfishcode/linux-raw-sys |
| litemap | 0.8.3 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| lock_api | 0.4.14 | MIT OR Apache-2.0 |  | https://github.com/Amanieu/parking_lot |
| log | 0.4.35 | MIT OR Apache-2.0 | oui | https://github.com/rust-lang/log |
| lzma-rust2 | 0.25.0 | Apache-2.0 |  | https://github.com/hasenbanck/lzma-rust2/ |
| matchers | 0.2.0 | MIT |  | https://github.com/hawkw/matchers |
| memchr | 2.8.3 | Unlicense OR MIT | oui | https://github.com/BurntSushi/memchr |
| memmap2 | 0.5.10 | MIT OR Apache-2.0 |  | https://github.com/RazrFalcon/memmap2-rs |
| mime | 0.3.18 | MIT OR Apache-2.0 |  | https://github.com/hyperium/mime |
| minimal-lexical | 0.2.1 | MIT/Apache-2.0 |  | https://github.com/Alexhuszagh/minimal-lexical |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 |  | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | oui | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| mio | 1.2.5 | MIT |  | https://github.com/tokio-rs/mio |
| multiversion_no_op | 1.0.0 | Apache-2.0 OR MIT |  | https://github.com/hsivonen/multiversion_no_op |
| nom | 7.1.3 | MIT |  | https://github.com/Geal/nom |
| nu-ansi-term | 0.50.3 | MIT |  | https://github.com/nushell/nu-ansi-term |
| num | 0.4.3 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num-bigint |
| num-complex | 0.4.6 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num-complex |
| num_cpus | 1.17.0 | MIT OR Apache-2.0 |  | https://github.com/seanmonstar/num_cpus |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |  | https://github.com/illicitonion/num_enum |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 |  | https://github.com/illicitonion/num_enum |
| num-integer | 0.1.47 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num-integer |
| num-iter | 0.1.46 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num-iter |
| num-rational | 0.4.2 | MIT OR Apache-2.0 |  | https://github.com/rust-num/num-rational |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | oui | https://github.com/rust-num/num-traits |
| ocrs | 0.14.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/ocrs |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | oui | https://github.com/matklad/once_cell |
| openssl-probe | 0.2.2 | MIT OR Apache-2.0 |  | https://github.com/rustls/openssl-probe |
| osmpbf | 0.3.8 | MIT OR Apache-2.0 |  | https://github.com/b-r-u/osmpbf |
| pastey | 0.2.3 | MIT OR Apache-2.0 |  | https://github.com/as1100k/pastey |
| pdqselect | 0.1.0 | Apache-2.0/MIT |  |  |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 |  | https://github.com/servo/rust-url/ |
| petgraph | 0.8.3 | MIT OR Apache-2.0 |  | https://github.com/petgraph/petgraph |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | oui | https://github.com/taiki-e/pin-project-lite |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/pkg-config-rs |
| plain | 0.2.3 | MIT/Apache-2.0 |  | https://github.com/randomites/plain |
| png | 0.18.1 | MIT OR Apache-2.0 |  | https://github.com/image-rs/image-png |
| potential_utf | 0.1.6 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| ppmd-rust | 1.5.2 | CC0-1.0 OR MIT-0 |  | https://github.com/hasenbanck/ppmd-rust |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/proc-macro2 |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 |  | https://github.com/bkchr/proc-macro-crate |
| proc-macro-error3 | 3.1.1 | MIT OR Apache-2.0 |  | https://github.com/gamma0987/proc-macro-error3 |
| proc-macro-error-attr3 | 3.1.1 | MIT OR Apache-2.0 |  | https://github.com/gamma0987/proc-macro-error3 |
| proj4rs | 0.3.0 | MIT OR Apache-2.0 | oui | https://github.com/3liz/proj4rs/ |
| proj4rs-geodesic | 0.2.0 | MIT OR Apache-2.0 | oui | https://github.com/3liz/proj4rs/ |
| proptest | 1.12.0 | MIT OR Apache-2.0 |  | https://github.com/proptest-rs/proptest |
| protobuf | 3.7.2 | MIT |  | https://github.com/stepancheg/rust-protobuf/ |
| protobuf-codegen | 3.7.2 | MIT |  | https://github.com/stepancheg/rust-protobuf/ |
| protobuf-parse | 3.7.2 | MIT |  | https://github.com/stepancheg/rust-protobuf/tree/master/protobuf-parse/ |
| protobuf-support | 3.7.2 | MIT |  | https://github.com/stepancheg/rust-protobuf/ |
| qrcode | 0.14.1 | MIT OR Apache-2.0 | oui | https://github.com/kennytm/qrcode-rust |
| quick-error | 2.0.1 | MIT/Apache-2.0 | oui | http://github.com/tailhook/quick-error |
| quick-xml | 0.42.0 | MIT | oui | https://github.com/tafia/quick-xml |
| quote | 1.0.47 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/quote |
| rand | 0.10.3 | MIT OR Apache-2.0 | oui | https://github.com/rust-random/rand |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | oui | https://github.com/rust-random/rand_core |
| rand_pcg | 0.10.2 | MIT OR Apache-2.0 | oui | https://github.com/rust-random/rngs |
| rand_xorshift | 0.5.0 | MIT OR Apache-2.0 |  | https://github.com/rust-random/rngs |
| rapidhash | 4.5.1 | MIT OR Apache-2.0 |  | https://github.com/hoxxep/rapidhash |
| rayon | 1.12.0 | MIT OR Apache-2.0 | oui | https://github.com/rayon-rs/rayon |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | oui | https://github.com/rayon-rs/rayon |
| regex | 1.13.1 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/regex |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/regex |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/regex |
| reqwest | 0.13.5 | MIT OR Apache-2.0 |  | https://github.com/seanmonstar/reqwest |
| ring | 0.17.14 | Apache-2.0 AND ISC |  | https://github.com/briansmith/ring |
| robust | 1.2.0 | MIT OR Apache-2.0 | oui | https://github.com/georust/robust |
| rstar | 0.8.4 | MIT/Apache-2.0 |  | https://github.com/Stoeoef/rstar |
| rstar | 0.9.3 | MIT/Apache-2.0 |  | https://github.com/georust/rstar |
| rstar | 0.10.0 | MIT OR Apache-2.0 |  | https://github.com/georust/rstar |
| rstar | 0.11.0 | MIT OR Apache-2.0 |  | https://github.com/georust/rstar |
| rstar | 0.12.2 | MIT OR Apache-2.0 | oui | https://github.com/georust/rstar |
| rstar | 0.13.1 | MIT OR Apache-2.0 |  | https://github.com/georust/rstar |
| rten | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-base | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-gemm | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-imageproc | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-onnx | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-parallel | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-sequence | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-shape-inference | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-simd | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-tensor | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rten-vecmath | 0.28.0 | MIT OR Apache-2.0 |  | https://github.com/robertknight/rten |
| rusqlite | 0.42.0 | MIT |  | https://github.com/rusqlite/rusqlite |
| rust-aec | 0.1.1 | MIT |  | https://github.com/hkwk/rust-aec |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT |  | https://github.com/rust-lang/rustc-hash |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 |  | https://github.com/djc/rustc-version-rs |
| rustix | 0.38.44 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |  | https://github.com/bytecodealliance/rustix |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | oui | https://github.com/bytecodealliance/rustix |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT |  | https://github.com/rustls/rustls |
| rustls-native-certs | 0.8.5 | Apache-2.0 OR ISC OR MIT |  | https://github.com/rustls/rustls-native-certs |
| rustls-pki-types | 1.16.0 | MIT OR Apache-2.0 |  | https://github.com/rustls/pki-types |
| rustls-platform-verifier | 0.7.1 | MIT OR Apache-2.0 |  | https://github.com/rustls/rustls-platform-verifier |
| rustls-platform-verifier-android | 0.2.0 | MIT OR Apache-2.0 |  | https://github.com/rustls/rustls-platform-verifier |
| rustls-webpki | 0.103.17 | ISC |  | https://github.com/rustls/webpki |
| rustversion | 1.0.23 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/rustversion |
| ryu | 1.0.23 | Apache-2.0 OR BSL-1.0 | oui | https://github.com/dtolnay/ryu |
| same-file | 1.0.6 | Unlicense/MIT |  | https://github.com/BurntSushi/same-file |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 |  | https://github.com/bluss/scopeguard |
| scroll | 0.12.0 | MIT |  | https://github.com/m4b/scroll |
| scroll_derive | 0.12.1 | MIT |  | https://github.com/m4b/scroll |
| semver | 1.0.28 | MIT OR Apache-2.0 | oui | https://github.com/dtolnay/semver |
| serde | 1.0.229 | MIT OR Apache-2.0 | oui | https://github.com/serde-rs/serde |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | oui | https://github.com/serde-rs/serde |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 |  | https://github.com/serde-rs/serde |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | oui | https://github.com/serde-rs/json |
| serde_spanned | 1.1.3 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| sevenz-rust2 | 0.25.0 | Apache-2.0 |  | https://github.com/hasenbanck/sevenz-rust |
| sha2 | 0.11.1 | MIT OR Apache-2.0 | oui | https://github.com/RustCrypto/hashes |
| sharded-slab | 0.1.7 | MIT |  | https://github.com/hawkw/sharded-slab |
| shlex | 2.0.1 | MIT OR Apache-2.0 |  | https://github.com/comex/rust-shlex |
| sif-itree | 0.4.1 | MIT OR Apache-2.0 | oui | https://codeberg.org/adamreichold/sif-itree |
| simd-adler32 | 0.3.10 | MIT | oui | https://github.com/mcountryman/simd-adler32 |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT |  | https://github.com/seancroach/simd_cesu8 |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 |  | https://github.com/rusticstuff/simdutf8 |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |  | https://github.com/jedisct1/rust-siphash |
| slab | 0.4.12 | MIT |  | https://github.com/tokio-rs/slab |
| smallvec | 1.16.5 | MIT OR Apache-2.0 | oui | https://github.com/servo/rust-smallvec |
| smawk | 0.3.3 | MIT |  | https://github.com/mgeisler/smawk |
| socket2 | 0.6.5 | MIT OR Apache-2.0 |  | https://github.com/rust-lang/socket2 |
| spade | 2.15.1 | MIT OR Apache-2.0 | oui | https://github.com/Stoeoef/spade |
| spin | 0.9.9 | MIT |  | https://github.com/mvdnes/spin-rs.git |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | oui | https://github.com/storyyeller/stable_deref_trait |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | oui | https://github.com/nvzqz/static-assertions-rs |
| strsim | 0.11.1 | MIT |  | https://github.com/rapidfuzz/strsim-rs |
| subtle | 2.6.1 | BSD-3-Clause |  | https://github.com/dalek-cryptography/subtle |
| syn | 2.0.119 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/syn |
| syn | 3.0.10 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/syn |
| sync_wrapper | 1.0.2 | Apache-2.0 |  | https://github.com/Actyx/sync_wrapper |
| synstructure | 0.14.0 | MIT |  | https://github.com/mystor/synstructure |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | oui | https://github.com/Stebalien/tempfile |
| textwrap | 0.16.4 | MIT |  | https://github.com/mgeisler/textwrap |
| thiserror | 1.0.69 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/thiserror |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | oui | https://github.com/dtolnay/thiserror |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/thiserror |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/thiserror |
| thread_local | 1.1.10 | MIT OR Apache-2.0 |  | https://github.com/Amanieu/thread_local-rs |
| tiff | 0.11.3 | MIT | oui | https://github.com/image-rs/image-tiff |
| tiny_http | 0.12.0 | MIT OR Apache-2.0 |  | https://github.com/tiny-http/tiny-http |
| tinystr | 0.8.5 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | oui | https://github.com/Lokathor/tinyvec |
| tokio | 1.54.1 | MIT |  | https://github.com/tokio-rs/tokio |
| tokio-rustls | 0.26.6 | MIT OR Apache-2.0 |  | https://github.com/rustls/tokio-rustls |
| tokio-util | 0.7.20 | MIT |  | https://github.com/tokio-rs/tokio |
| toml | 1.2.1+spec-1.1.0 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| toml_datetime | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| toml_edit | 0.25.19+spec-1.1.0 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| toml_writer | 1.2.0+spec-1.1.0 | MIT OR Apache-2.0 |  | https://github.com/toml-rs/toml |
| tower | 0.5.3 | MIT |  | https://github.com/tower-rs/tower |
| tower-http | 0.6.11 | MIT |  | https://github.com/tower-rs/tower-http |
| tower-layer | 0.3.3 | MIT |  | https://github.com/tower-rs/tower |
| tower-service | 0.3.3 | MIT |  | https://github.com/tower-rs/tower |
| tracing | 0.1.44 | MIT | oui | https://github.com/tokio-rs/tracing |
| tracing-attributes | 0.1.31 | MIT |  | https://github.com/tokio-rs/tracing |
| tracing-core | 0.1.36 | MIT | oui | https://github.com/tokio-rs/tracing |
| tracing-log | 0.2.0 | MIT |  | https://github.com/tokio-rs/tracing |
| tracing-subscriber | 0.3.23 | MIT |  | https://github.com/tokio-rs/tracing |
| try-lock | 0.2.5 | MIT |  | https://github.com/seanmonstar/try-lock |
| twox-hash | 2.1.5 | MIT | oui | https://github.com/shepmaster/twox-hash |
| typed-path | 0.13.0 | MIT OR Apache-2.0 | oui | https://github.com/chipsenkbeil/typed-path |
| typeid | 1.0.3 | MIT OR Apache-2.0 |  | https://github.com/dtolnay/typeid |
| typenum | 1.20.1 | MIT OR Apache-2.0 | oui | https://github.com/paholg/typenum |
| unarray | 0.1.4 | MIT OR Apache-2.0 |  | https://github.com/cameron1024/unarray |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |  | https://github.com/dtolnay/unicode-ident |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 |  | https://github.com/unicode-rs/unicode-width |
| uniffi | 0.32.2 | MPL-2.0 | oui | https://github.com/mozilla/uniffi-rs |
| uniffi_bindgen | 0.32.2 | MPL-2.0 |  | https://github.com/mozilla/uniffi-rs |
| uniffi_core | 0.32.2 | MPL-2.0 | oui | https://github.com/mozilla/uniffi-rs |
| uniffi_internal_macros | 0.32.2 | MPL-2.0 |  | https://github.com/mozilla/uniffi-rs |
| uniffi_macros | 0.32.2 | MPL-2.0 |  | https://github.com/mozilla/uniffi-rs |
| uniffi_meta | 0.32.2 | MPL-2.0 |  | https://github.com/mozilla/uniffi-rs |
| uniffi_pipeline | 0.32.2 | MPL-2.0 | oui | https://github.com/mozilla/uniffi-rs |
| uniffi_udl | 0.32.2 | MPL-2.0 |  | https://github.com/mozilla/uniffi-rs |
| untrusted | 0.9.0 | ISC |  | https://github.com/briansmith/untrusted |
| unty-next | 0.1.2 | MIT OR Apache-2.0 |  | https://github.com/Apich-Organization/unty |
| url | 2.5.8 | MIT OR Apache-2.0 |  | https://github.com/servo/rust-url |
| utf8_iter | 1.1.0 | Apache-2.0 OR MIT |  | https://github.com/hsivonen/utf8_iter |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT |  | https://github.com/alacritty/vte |
| utf_types | 1.0.0 | Apache-2.0 OR MIT |  | https://github.com/hsivonen/utf_types |
| vcpkg | 0.2.15 | MIT/Apache-2.0 |  | https://github.com/mcgoo/vcpkg-rs |
| version_check | 0.9.5 | MIT/Apache-2.0 |  | https://github.com/SergioBenitez/version_check |
| walkdir | 2.5.0 | Unlicense/MIT |  | https://github.com/BurntSushi/walkdir |
| want | 0.3.2 | MIT |  | https://github.com/seanmonstar/want |
| webpki-roots | 1.0.12 | CDLA-Permissive-2.0 |  | https://github.com/rustls/webpki-roots |
| weedle2 | 5.0.0 | MIT |  | https://github.com/mozilla/uniffi-rs |
| weezl | 0.1.12 | MIT OR Apache-2.0 | oui | https://github.com/image-rs/weezl |
| which | 4.4.2 | MIT |  | https://github.com/harryfei/which-rs.git |
| winnow | 1.0.5 | MIT |  | https://github.com/winnow-rs/winnow |
| writeable | 0.6.4 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| yoke | 0.8.4 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| yoke-derive | 0.8.4 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | oui | https://github.com/google/zerocopy |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT |  | https://github.com/google/zerocopy |
| zerofrom | 0.1.8 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zerofrom-derive | 0.1.8 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | oui | https://github.com/RustCrypto/utils |
| zeroize_derive | 1.5.0 | Apache-2.0 OR MIT |  | https://github.com/RustCrypto/utils |
| zerotrie | 0.2.5 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zerovec | 0.11.9 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zerovec-derive | 0.11.7 | Unicode-3.0 |  | https://github.com/unicode-org/icu4x |
| zip | 9.5.1 | MIT | oui | https://github.com/zip-rs/zip2 |
| zlib-rs | 0.6.8 | Zlib | oui | https://github.com/trifectatechfoundation/zlib-rs |
| zmij | 1.0.23 | MIT | oui | https://github.com/dtolnay/zmij |
| zopfli | 0.8.4 | Apache-2.0 | oui | https://github.com/zopfli-rs/zopfli |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | oui | https://github.com/etemesi254/zune-image |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | oui | https://github.com/etemesi254/zune-image/tree/dev/crates/zune-jpeg |

## Bibliothèques Android

169 bibliothèques dans le classpath d'exécution de la version release de l'app (`:app`, `releaseRuntimeClasspath`), licence lue dans leur POM (plusieurs licences séparées par « ; » : toutes celles que le POM déclare) ; licence absente du POM : 0.

| Bibliothèque | Version | Licence (POM) |
|---|---|---|
| androidx.activity:activity | 1.13.0 | The Apache Software License, Version 2.0 |
| androidx.activity:activity-compose | 1.13.0 | The Apache Software License, Version 2.0 |
| androidx.activity:activity-ktx | 1.13.0 | The Apache Software License, Version 2.0 |
| androidx.annotation:annotation | 1.10.0 | The Apache Software License, Version 2.0 |
| androidx.annotation:annotation-experimental | 1.4.1 | The Apache Software License, Version 2.0 |
| androidx.annotation:annotation-jvm | 1.10.0 | The Apache Software License, Version 2.0 |
| androidx.appcompat:appcompat | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.appcompat:appcompat-resources | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.arch.core:core-common | 2.2.0 | The Apache Software License, Version 2.0 |
| androidx.arch.core:core-runtime | 2.2.0 | The Apache Software License, Version 2.0 |
| androidx.autofill:autofill | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.camera:camera-camera2 | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera:camera-camera2-pipe | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera:camera-core | 1.6.2 | The Apache Software License, Version 2.0 ; BSD-3-Clause |
| androidx.camera:camera-lifecycle | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera:camera-video | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera:camera-view | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera.featurecombinationquery:featurecombinationquery | 1.6.2 | The Apache Software License, Version 2.0 |
| androidx.camera.viewfinder:viewfinder-core | 1.5.1 | The Apache Software License, Version 2.0 |
| androidx.collection:collection | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.collection:collection-jvm | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.collection:collection-ktx | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.compose.animation:animation | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.animation:animation-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.animation:animation-core | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.animation:animation-core-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose:compose-bom | 2026.09.00 | The Apache Software License, Version 2.0 |
| androidx.compose.foundation:foundation | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.foundation:foundation-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.foundation:foundation-layout | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.foundation:foundation-layout-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.material3:material3 | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.compose.material3:material3-android | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.compose.material:material-ripple | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.material:material-ripple-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-annotation | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-annotation-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-retain | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-retain-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-saveable | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.runtime:runtime-saveable-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-geometry | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-geometry-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-graphics | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-graphics-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-text | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-text-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-tooling-preview | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-tooling-preview-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-unit | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-unit-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-util | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.compose.ui:ui-util-android | 1.12.1 | The Apache Software License, Version 2.0 |
| androidx.concurrent:concurrent-futures | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.concurrent:concurrent-futures-ktx | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.core:core | 1.19.1 | The Apache Software License, Version 2.0 |
| androidx.core:core-backported-fixes | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.core:core-ktx | 1.19.1 | The Apache Software License, Version 2.0 |
| androidx.core:core-viewtree | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.cursoradapter:cursoradapter | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.customview:customview | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.customview:customview-poolingcontainer | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.drawerlayout:drawerlayout | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.emoji2:emoji2 | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.exifinterface:exifinterface | 1.4.2 | The Apache Software License, Version 2.0 |
| androidx.fragment:fragment | 1.8.9 | The Apache Software License, Version 2.0 |
| androidx.graphics:graphics-path | 1.0.1 | The Apache Software License, Version 2.0 |
| androidx.health.connect:connect-client | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.health.connect:connect-client-external-protobuf | 1.1.0 | BSD-3-Clause |
| androidx.health.connect:connect-client-proto | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.interpolator:interpolator | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-common | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-common-java8 | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-common-jvm | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-livedata | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-livedata-core | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-livedata-core-ktx | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-process | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime-compose | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime-compose-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime-ktx | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-runtime-ktx-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-service | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-compose | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-compose-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-ktx | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-savedstate | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.lifecycle:lifecycle-viewmodel-savedstate-android | 2.11.0 | The Apache Software License, Version 2.0 |
| androidx.loader:loader | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.media3:media3-common | 1.9.0 | The Apache Software License, Version 2.0 |
| androidx.media3:media3-container | 1.9.0 | The Apache Software License, Version 2.0 |
| androidx.media3:media3-muxer | 1.9.0 | The Apache Software License, Version 2.0 |
| androidx.navigationevent:navigationevent | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.navigationevent:navigationevent-android | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.navigationevent:navigationevent-compose | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.navigationevent:navigationevent-compose-android | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.profileinstaller:profileinstaller | 1.4.1 | The Apache Software License, Version 2.0 |
| androidx.room:room-common | 2.7.0 | The Apache Software License, Version 2.0 |
| androidx.room:room-common-jvm | 2.7.0 | The Apache Software License, Version 2.0 |
| androidx.room:room-runtime | 2.7.0 | The Apache Software License, Version 2.0 |
| androidx.room:room-runtime-android | 2.7.0 | The Apache Software License, Version 2.0 |
| androidx.savedstate:savedstate | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.savedstate:savedstate-android | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.savedstate:savedstate-compose | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.savedstate:savedstate-compose-android | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.savedstate:savedstate-ktx | 1.4.0 | The Apache Software License, Version 2.0 |
| androidx.sqlite:sqlite | 2.5.0 | The Apache Software License, Version 2.0 |
| androidx.sqlite:sqlite-android | 2.5.0 | The Apache Software License, Version 2.0 |
| androidx.sqlite:sqlite-framework | 2.5.0 | The Apache Software License, Version 2.0 |
| androidx.sqlite:sqlite-framework-android | 2.5.0 | The Apache Software License, Version 2.0 |
| androidx.startup:startup-runtime | 1.1.1 | The Apache Software License, Version 2.0 |
| androidx.tracing:tracing | 1.3.0 | The Apache Software License, Version 2.0 |
| androidx.tracing:tracing-android | 1.3.0 | The Apache Software License, Version 2.0 |
| androidx.tracing:tracing-ktx | 1.3.0 | The Apache Software License, Version 2.0 |
| androidx.vectordrawable:vectordrawable | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.vectordrawable:vectordrawable-animated | 1.1.0 | The Apache Software License, Version 2.0 |
| androidx.versionedparcelable:versionedparcelable | 1.1.1 | The Apache Software License, Version 2.0 |
| androidx.viewpager:viewpager | 1.0.0 | The Apache Software License, Version 2.0 |
| androidx.window:window | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.window:window-core | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.window:window-core-android | 1.5.0 | The Apache Software License, Version 2.0 |
| androidx.work:work-runtime | 2.12.0 | The Apache Software License, Version 2.0 |
| com.google.auto.value:auto-value-annotations | 1.6.3 | Apache 2.0 |
| com.google.code.findbugs:jsr305 | 3.0.2 | The Apache Software License, Version 2.0 |
| com.google.code.gson:gson | 2.10.1 | Apache-2.0 |
| com.google.dagger:dagger | 2.59 | Apache 2.0 |
| com.google.errorprone:error_prone_annotations | 2.28.0 | Apache 2.0 |
| com.google.guava:failureaccess | 1.0.2 | The Apache Software License, Version 2.0 |
| com.google.guava:guava | 33.3.1-android | Apache License, Version 2.0 |
| com.google.guava:listenablefuture | 9999.0-empty-to-avoid-conflict-with-guava | The Apache Software License, Version 2.0 |
| com.google.j2objc:j2objc-annotations | 3.0.0 | Apache License, Version 2.0 |
| com.google.zxing:core | 3.5.4 | The Apache Software License, Version 2.0 |
| com.jakewharton.timber:timber | 5.0.1 | The Apache Software License, Version 2.0 |
| com.squareup.okhttp3:okhttp | 4.12.0 | The Apache Software License, Version 2.0 |
| com.squareup.okio:okio | 3.6.0 | The Apache Software License, Version 2.0 |
| com.squareup.okio:okio-jvm | 3.6.0 | The Apache Software License, Version 2.0 |
| jakarta.inject:jakarta.inject-api | 2.0.1 | The Apache Software License, Version 2.0 |
| javax.inject:javax.inject | 1 | The Apache Software License, Version 2.0 |
| net.java.dev.jna:jna | 5.19.1 | LGPL-2.1-or-later ; Apache-2.0 |
| org.checkerframework:checker-qual | 3.43.0 | The MIT License |
| org.jetbrains:annotations | 23.0.0 | The Apache Software License, Version 2.0 |
| org.jetbrains.kotlin:kotlin-bom | 1.8.0 | The Apache Software License, Version 2.0 |
| org.jetbrains.kotlin:kotlin-stdlib | 2.4.21 | Apache-2.0 |
| org.jetbrains.kotlin:kotlin-stdlib-common | 2.4.21 | Apache-2.0 |
| org.jetbrains.kotlin:kotlin-stdlib-jdk7 | 1.9.10 | The Apache License, Version 2.0 |
| org.jetbrains.kotlin:kotlin-stdlib-jdk8 | 1.9.10 | The Apache License, Version 2.0 |
| org.jetbrains.kotlinx:atomicfu | 0.28.0 | Apache-2.0 |
| org.jetbrains.kotlinx:atomicfu-jvm | 0.28.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-coroutines-android | 1.11.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-coroutines-bom | 1.11.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-coroutines-core | 1.11.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-coroutines-core-jvm | 1.11.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-coroutines-guava | 1.11.0 | Apache-2.0 |
| org.jetbrains.kotlinx:kotlinx-serialization-bom | 1.7.3 | The Apache Software License, Version 2.0 |
| org.jetbrains.kotlinx:kotlinx-serialization-core | 1.7.3 | The Apache Software License, Version 2.0 |
| org.jetbrains.kotlinx:kotlinx-serialization-core-jvm | 1.7.3 | The Apache Software License, Version 2.0 |
| org.jspecify:jspecify | 1.0.0 | The Apache License, Version 2.0 |
| org.maplibre.gl:android-sdk | 13.7.1 | BSD |
| org.maplibre.gl:android-sdk-geojson | 6.0.1 | The Apache Software License, Version 2.0 |
| org.maplibre.gl:android-sdk-turf | 6.0.1 | The Apache Software License, Version 2.0 |
| org.maplibre.gl:maplibre-android-gestures | 0.0.4 | BSD |
