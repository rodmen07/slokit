//! The sloth upstream corpus contract (v1.7.0 PR 1).
//!
//! slokit has claimed sloth `prometheus/v1` compatibility since 0.1.0 and
//! widened it one dialect at a time, each widening grounded on the handful of
//! upstream documents that happened to fail. This file is the first time the
//! **whole** upstream corpus is committed and pinned: every document of
//! `slok/sloth`'s `examples/` tree, its upstream sha256, and the exact
//! disposition slokit gives it. A change in any of those three is a test
//! failure with a name, not a surprise in the next release.
//!
//! **Updated by v1.7.0 PR 2.** The two `windows/` rows moved from `Refused`
//! to `Accepted` when `kind: AlertWindows` became readable. The table is the
//! record of that: what changed is two words here, and everything else about
//! the corpus — the paths, the hashes, the other 18 dispositions — is asserted
//! to be unchanged by the same run.
//!
//! **Why the fixtures live in a subdirectory.** `tests/schema.rs`'s
//! `native_fixture_files()` (`tests/schema.rs:531`) reads `tests/fixtures`
//! **non-recursively** and validates everything it finds against the native
//! JSON Schema. Nine of these 20 documents are not native specs at all (sloth
//! CRDs, OpenSLO documents, `kind: AlertWindows` catalogues), so dropping them
//! at the fixtures root would redden the schema suite on the very commit that
//! adds the files. `sloth_corpus/` is invisible to that walker, the same
//! discharge `sloth_crd/` got in v1.6.0.
//!
//! **Why the expectation table is hand-written while the file list is not.**
//! The 20 paths are glob-discovered (recursively, with a hard failure on a
//! zero-length walk), and then reconciled against [`CORPUS`] in **both**
//! directions: a fixture with no entry fails, and an entry with no fixture
//! fails. So the table cannot silently stop covering the tree, and the tree
//! cannot silently stop matching the table. The hashes and dispositions
//! themselves have to be written down — that is the contract.
//!
//! **Why the hasher is implemented here.** The recorded sha256 is what makes
//! this guard un-green-able by editing a fixture, and slokit has no hashing
//! dependency (`cargo audit` surface and the lean core are both deliberate).
//! [`sha256_hex`] is a ~60-line FIPS 180-4 implementation pinned by
//! [`the_hasher_reproduces_the_nist_vectors`], because a hasher that returned
//! a constant would make every hash assertion below vacuously green.

#![cfg(feature = "cli")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The upstream tree these fixtures were fetched from. Recorded so a refresh
/// is a two-line diff (commit here, hashes in [`CORPUS`]) rather than an
/// archaeology exercise.
const UPSTREAM_REPO: &str = "slok/sloth";
const UPSTREAM_COMMIT: &str = "8a3be4fab79defa4448d09d91b48422615980b05";

/// Where the committed copies live, relative to the crate root.
const CORPUS_DIR: &str = "tests/fixtures/sloth_corpus";

/// What slokit does with one upstream document today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// `slokit validate -i <doc>` exits 0.
    Accepted,
    /// `slokit validate -i <doc>` exits non-zero, and its output contains this
    /// substring. The substring is the part of the message that carries the
    /// REASON, so a refusal that starts happening for a different reason fails
    /// here instead of passing as "still refused".
    Refused(&'static str),
}

use Disposition::{Accepted, Refused};

/// One upstream document and everything this contract pins about it.
struct Entry {
    /// Path under [`CORPUS_DIR`], with `/` separators.
    path: &'static str,
    /// sha256 of the upstream bytes. `.gitattributes` pins `eol=lf` for this
    /// directory so the checked-out bytes are the upstream bytes on every
    /// platform; without that, a Windows checkout under `core.autocrlf=true`
    /// would rewrite every file and this column would be unverifiable.
    sha256: &'static str,
    /// The disposition slokit gives it.
    disposition: Disposition,
    /// Lint codes that MUST appear when slokit accepts the document but
    /// discards part of it. Empty for a document nothing is dropped from.
    ///
    /// This column is why "accepted" stays an honest word here: two documents
    /// below are accepted with a sloth SLO plugin chain silently discarded,
    /// and a contract that recorded only `Accepted` for them would be
    /// describing a lossless import that does not exist.
    lint_codes: &'static [&'static str],
}

/// Every document in `slok/sloth@<UPSTREAM_COMMIT>`'s `examples/` tree: the 18
/// files at its root plus the 2 under `examples/windows/`. The three
/// directories `_gen`, `plugins` and `windows` were identified by type from
/// the contents API; `_gen` holds sloth's own OUTPUT and `plugins` holds Go
/// source, so neither is an input document and neither is committed here.
const CORPUS: &[Entry] = &[
    Entry {
        path: "contrib-denominator-corrected.yaml",
        sha256: "7bda3fceef2ca67684c9c0e757bd2cc73e38bde69a0ebad4a27c88b372b9038b",
        disposition: Refused("spec.sloPlugins is a sloth SLO plugin chain"),
        lint_codes: &[],
    },
    Entry {
        path: "contrib-slo-plugins.yml",
        sha256: "08b7d9c3ade7b8a37027cdfdfa50320f786862595b421260302e198114b97927",
        disposition: Accepted,
        lint_codes: &["SLO_PLUGIN_CHAIN_DROPPED"],
    },
    Entry {
        path: "getting-started.yml",
        sha256: "dfcaf9be76683dc838ed81e53b1b8876d11996cee24e78e47775bca24d0d4588",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "home-wifi.yml",
        sha256: "f0bb1ebd3936c4eef1579091a4f35d079571f11697005d805b02b0bfe77dd5c1",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "k8s-getting-started.yml",
        sha256: "3f17c6c1163ac2c57710ced7b114f07b23aa6ea6fa4e67de850563ee1f111d14",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "k8s-home-wifi.yml",
        sha256: "3094c6c876e81afffcc35812d997c8a270775bfa5490901f65960d3624c10502",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "k8s-multifile.yml",
        sha256: "0807130bb8a95eda84db55b68a30f640f56ae5a8df6fcd31f3663d610a130da4",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "kubernetes-apiserver.yml",
        sha256: "b910fb1df93eaefe3c257c314519701b2dd1e6d206aa72b10cd6995559fb02aa",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "multifile.yml",
        sha256: "9c7b1b5ea0c0109b25f1fbf2bd6da31fec210bbf796442bec46329f7cb882fac",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "no-alerts.yml",
        sha256: "1a8ddbf65efa0cfdc8bed0ae517ef9aa2805906f876f30847382cf93b8679da2",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "openslo-getting-started.yml",
        sha256: "b64679a907d62f3c3413b7d63a6df98921ea34be0ca8f2b71e5c623874de8a1e",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "openslo-kubernetes-apiserver.yml",
        sha256: "6e86711de93095dcfab81b587d33eab3502f503ff6c39c06c0e94cdcd482f9c2",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "plugin-getting-started.yml",
        sha256: "5d1be2f330970f440dd97581e9365c6390ae70380116a904a8fa70b03be98649",
        // Correct: the SLI plugin is a Go plugin under `examples/plugins/`,
        // which sloth itself only resolves with `--sli-plugins-path`.
        disposition: Refused("unknown SLI plugin 'getting_started_availability'"),
        lint_codes: &[],
    },
    Entry {
        path: "plugin-k8s-getting-started.yml",
        sha256: "898be2d4bcd619302ea71fa54c2b7c138e2bf9c9cfcf803b561f46930e35ef02",
        // Correct: upstream's own `page_alert` spelling bug in a CRD document,
        // where the field is `pageAlert`.
        disposition: Refused("must write alerting.pageAlert"),
        lint_codes: &[],
    },
    Entry {
        path: "raw-home-wifi.yml",
        sha256: "773151502e15c41672fc4ff10ac1c20016c8e04175875de49e168416af801635",
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "slo-plugin-getting-started.yml",
        sha256: "8cb3055e9900f4335e33edec71964a83f12fb1bd9c7daec4b19a71f8ff7cf62d",
        disposition: Accepted,
        lint_codes: &["SLO_PLUGIN_CHAIN_DROPPED"],
    },
    Entry {
        path: "slo-plugin-k8s-getting-started.yml",
        sha256: "82586b28604660580b2855aa5a81f432e764fe1a01611db8e34a4a91e3a4d5dd",
        disposition: Refused("spec.sloPlugins is a sloth SLO plugin chain"),
        lint_codes: &[],
    },
    Entry {
        path: "victoria-metrics.yml",
        sha256: "27612ca6308fee5eba0ff22109993d6f8f4d097c32836ebfc64ed58552f6e297",
        // Rejected before v1.7.0 with `invalid type: boolean 'true', expected
        // a string`, over nine unquoted label scalars. See
        // `the_victoria_metrics_label_scalars_reach_the_generated_rules`.
        //
        // It carries a four-entry `slo_plugins` chain too, which the v1.7.0
        // scoping census did not record: the census could only observe this
        // document being REFUSED, so the second defect inside it was invisible
        // until the label fix made it parse. That is why the milestone's
        // done-when clause 3 said "both native upstream documents that carry
        // one" and now says three.
        disposition: Accepted,
        lint_codes: &["SLO_PLUGIN_CHAIN_DROPPED"],
    },
    Entry {
        path: "windows/7d.yaml",
        sha256: "dca7a946901575a1b3f4beb95ad7387bc50f31251a3aa5bc5f0859abf478a56f",
        // FLIPPED by v1.7.0 PR 2, which added `kind: AlertWindows`. It was
        // `Refused("no kind: PrometheusServiceLevel documents in input")`:
        // auto-detection routed on the `apiVersion` group alone, handed the
        // document to the CRD importer, and nothing there knew the kind.
        // `validate` now reads it as a burn-rate window catalogue and reports
        // the factors it would apply (13.44 / 3.5 / 1.4 / 0.98); the rules
        // those factors produce are asserted in `tests/alert_windows_cli.rs`.
        //
        // The empty `lint_codes` is a claim, not a default: nothing in this
        // document is discarded. A catalogue is applied whole or refused —
        // every one of its four blocks is required, so there is no partial
        // acceptance to report.
        disposition: Accepted,
        lint_codes: &[],
    },
    Entry {
        path: "windows/custom-30d.yaml",
        sha256: "b9c08ec3efe5b218e13283f6c3460aedcd4d87004550866508c114f8018fb566",
        // Flipped by the same PR; factors 14.4 / 4.8 / 3 / 1 over the
        // 30m / 3h / 12h / 36h lookbacks it declares.
        disposition: Accepted,
        lint_codes: &[],
    },
];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_DIR)
}

fn slokit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_slokit"))
        .args(args)
        .output()
        .expect("slokit binary runs")
}

/// Everything the binary wrote, both streams, as one searchable string.
fn combined(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// Every regular file under `dir`, recursively, as `/`-separated paths
/// relative to [`corpus_root`].
///
/// Recursive on purpose: `windows/` is a real subdirectory of the upstream
/// tree, and a non-recursive walk would quietly cover 18 of 20 documents.
fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("corpus directory {} is readable: {e}", dir.display()))
        .map(|e| e.expect("readable dir entry").path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .expect("dir entry has a name")
            .to_string_lossy()
            .to_string();
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk(&path, &rel, out);
        } else {
            out.push(rel);
        }
    }
}

fn discovered_fixtures() -> Vec<String> {
    let mut found = Vec::new();
    walk(&corpus_root(), "", &mut found);
    assert!(
        !found.is_empty(),
        "zero files discovered under {}; every assertion in this file would be \
         vacuously green, so a moved or emptied corpus fails here instead",
        CORPUS_DIR
    );
    found.sort();
    found
}

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4), so the contract needs no hashing dependency.
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Hex sha256 of `data`.
fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }

        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }

    h.iter().map(|w| format!("{w:08x}")).collect()
}

// ---------------------------------------------------------------------------
// Done-when clause 1: the corpus contract
// ---------------------------------------------------------------------------

/// The hasher is the load-bearing part of every hash assertion below, so it is
/// pinned against published vectors first. A `sha256_hex` that returned a
/// constant, or truncated, would make [`every_fixture_still_hashes_to_its_recorded_value`]
/// green no matter what the fixtures contained.
#[test]
fn the_hasher_reproduces_the_nist_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "sha256 of the empty string"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha256 of \"abc\""
    );
    // Two blocks, so the multi-block path is exercised too.
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        "sha256 of the 56-byte NIST vector"
    );
}

/// The committed tree and [`CORPUS`] must describe the same set of files, in
/// both directions. Adding a fixture without a contract row, or dropping a
/// fixture a row still claims, fails here by name.
#[test]
fn the_contract_covers_exactly_the_committed_corpus() {
    let found = discovered_fixtures();
    let mut listed: Vec<String> = CORPUS.iter().map(|e| e.path.to_string()).collect();
    listed.sort();

    let unlisted: Vec<&String> = found.iter().filter(|p| !listed.contains(p)).collect();
    assert!(
        unlisted.is_empty(),
        "committed under {CORPUS_DIR} but absent from CORPUS: {unlisted:?}"
    );
    let missing: Vec<&String> = listed.iter().filter(|p| !found.contains(p)).collect();
    assert!(
        missing.is_empty(),
        "listed in CORPUS but not committed under {CORPUS_DIR}: {missing:?}"
    );

    assert_eq!(
        found.len(),
        20,
        "the upstream examples/ tree at {UPSTREAM_REPO}@{UPSTREAM_COMMIT} holds 20 documents \
         (18 at the root plus 2 under windows/); found {}",
        found.len()
    );
}

/// The recorded hash is what stops this contract from being made green by
/// editing a fixture: a document whose disposition is inconvenient cannot be
/// quietly adjusted, because its hash stops matching upstream.
#[test]
fn every_fixture_still_hashes_to_its_recorded_value() {
    let root = corpus_root();
    let mut drifted = Vec::new();
    for entry in CORPUS {
        let path = root.join(entry.path);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let actual = sha256_hex(&bytes);
        if actual != entry.sha256 {
            drifted.push(format!(
                "{}: recorded {} but the committed bytes hash to {actual}",
                entry.path, entry.sha256
            ));
        }
    }
    assert!(
        drifted.is_empty(),
        "fixture bytes no longer match the upstream sha256 recorded from \
         {UPSTREAM_REPO}@{UPSTREAM_COMMIT}:\n  {}",
        drifted.join("\n  ")
    );
}

/// The contract proper: what slokit does with each upstream document.
#[test]
fn every_fixture_has_its_recorded_disposition() {
    let mut wrong = Vec::new();
    for entry in CORPUS {
        let path = format!("{CORPUS_DIR}/{}", entry.path);
        let out = slokit(&["validate", "-i", &path]);
        let text = combined(&out);
        match entry.disposition {
            Accepted => {
                if !out.status.success() {
                    wrong.push(format!(
                        "{}: recorded Accepted, but validate exited {:?}:\n      {}",
                        entry.path,
                        out.status.code(),
                        text.trim().replace('\n', "\n      ")
                    ));
                }
            }
            Refused(reason) => {
                if out.status.success() {
                    wrong.push(format!(
                        "{}: recorded Refused({reason:?}), but validate exited 0",
                        entry.path
                    ));
                } else if !text.contains(reason) {
                    wrong.push(format!(
                        "{}: refused, but for a different reason than {reason:?}:\n      {}",
                        entry.path,
                        text.trim().replace('\n', "\n      ")
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "corpus dispositions drifted:\n  {}",
        wrong.join("\n  ")
    );
}

/// A document recorded as `Accepted` while part of it is discarded must say so
/// through `slokit lint`, and a document with no `lint_codes` must NOT emit
/// them. Both directions matter: without the second half, a rule that fired on
/// everything would satisfy the first.
#[test]
fn the_lossy_acceptances_are_the_ones_lint_reports() {
    let mut wrong = Vec::new();
    for entry in CORPUS {
        if entry.disposition != Accepted {
            continue;
        }
        let path = format!("{CORPUS_DIR}/{}", entry.path);
        let text = combined(&slokit(&["lint", "-i", &path]));
        for code in entry.lint_codes {
            if !text.contains(code) {
                wrong.push(format!(
                    "{}: expected lint {code}, got:\n      {}",
                    entry.path,
                    text.trim().replace('\n', "\n      ")
                ));
            }
        }
        if !entry.lint_codes.contains(&"SLO_PLUGIN_CHAIN_DROPPED")
            && text.contains("SLO_PLUGIN_CHAIN_DROPPED")
        {
            wrong.push(format!(
                "{}: SLO_PLUGIN_CHAIN_DROPPED fired on a document that carries no chain",
                entry.path
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "lossy-acceptance reporting drifted:\n  {}",
        wrong.join("\n  ")
    );
}

// ---------------------------------------------------------------------------
// Done-when clause 2: the label scalars reach the rules
// ---------------------------------------------------------------------------

/// Parsing `victoria-metrics.yml` is not the claim — a coercion that parsed
/// the document and then dropped the labels would satisfy a weaker one. Each
/// of the nine values upstream leaves unquoted is asserted as a LABEL VALUE on
/// the rules the binary emits, in its canonical string form.
#[test]
fn the_victoria_metrics_label_scalars_reach_the_generated_rules() {
    let out = slokit(&[
        "generate",
        "-i",
        &format!("{CORPUS_DIR}/victoria-metrics.yml"),
    ]);
    assert!(
        out.status.success(),
        "generate failed: {}",
        combined(&out).trim()
    );
    let rules = String::from_utf8(out.stdout).expect("generated rules are utf-8");

    // Exactly the nine values upstream writes unquoted: one bool in the
    // service label map, two floats per SLO in the SLO label map, and two ints
    // per SLO in alerting.labels.
    let expected = [
        ("generated", "true"),
        ("actual_le", "0.2"),
        ("target_le", "0.2"),
        ("objective", "90"),
        ("objective_reversed", "10"),
        ("actual_le", "0.4"),
        ("target_le", "0.4"),
        ("objective", "99"),
        ("objective_reversed", "1"),
    ];
    for (name, value) in expected {
        let rendered = format!("{name}: '{value}'");
        assert!(
            rules.contains(&rendered),
            "generated rules carry no `{rendered}`; the unquoted YAML scalar was \
             parsed but did not survive to the rules"
        );
    }
}

// ---------------------------------------------------------------------------
// Done-when clause 3: the dropped plugin chain is reported
// ---------------------------------------------------------------------------

/// Every native upstream document that carries a sloth SLO plugin chain must
/// produce a `SLO_PLUGIN_CHAIN_DROPPED` finding, and the message must name the
/// SPELLING that carried it, because `slo_plugins` (spec level) and `plugins`
/// (SLO level) are two different keys a reader has to go and delete.
///
/// There are THREE such documents, not the two the v1.7.0 scoping census
/// recorded: `victoria-metrics.yml` carries one as well, and the census could
/// not see it because that document was refused at parse time for an unrelated
/// reason. `the_lossy_acceptances_are_the_ones_lint_reports` is what found it,
/// by asserting the negative direction on every accepted document rather than
/// only the positive one on the two that were expected.
#[test]
fn the_dropped_plugin_chain_is_reported_on_every_native_document_that_carries_one() {
    let text = combined(&slokit(&[
        "lint",
        "-i",
        &format!("{CORPUS_DIR}/slo-plugin-getting-started.yml"),
    ]));
    assert!(
        text.contains("SLO_PLUGIN_CHAIN_DROPPED") && text.contains("`slo_plugins`"),
        "spec-level chain unreported in:\n{text}"
    );
    assert!(
        text.contains("`plugins`"),
        "SLO-level chain unreported in:\n{text}"
    );
    // Two findings, one per spelling — a rule that reported only the first
    // would leave the reader deleting one key and wondering why nothing
    // changed.
    assert_eq!(
        text.matches("SLO_PLUGIN_CHAIN_DROPPED").count(),
        2,
        "expected one finding per chain in:\n{text}"
    );

    let text = combined(&slokit(&[
        "lint",
        "-i",
        &format!("{CORPUS_DIR}/contrib-slo-plugins.yml"),
    ]));
    assert_eq!(
        text.matches("SLO_PLUGIN_CHAIN_DROPPED").count(),
        2,
        "contrib-slo-plugins.yml carries a chain on each of its two SLOs:\n{text}"
    );

    let text = combined(&slokit(&[
        "lint",
        "-i",
        &format!("{CORPUS_DIR}/victoria-metrics.yml"),
    ]));
    assert_eq!(
        text.matches("SLO_PLUGIN_CHAIN_DROPPED").count(),
        1,
        "victoria-metrics.yml carries a four-entry spec-level chain:\n{text}"
    );
    assert!(
        text.contains("4 entries: sloth.dev/contrib/validate_victoria_metrics/v1"),
        "the finding must enumerate the chain it is reporting:\n{text}"
    );

    // The set is derived, not asserted from memory: exactly these three
    // documents of the twenty carry a chain, so a fourth appearing upstream
    // (or one of these losing its chain) fails here rather than passing as
    // "the two we knew about are still reported".
    let carriers: Vec<&str> = CORPUS
        .iter()
        .filter(|e| e.lint_codes.contains(&"SLO_PLUGIN_CHAIN_DROPPED"))
        .map(|e| e.path)
        .collect();
    assert_eq!(
        carriers,
        vec![
            "contrib-slo-plugins.yml",
            "slo-plugin-getting-started.yml",
            "victoria-metrics.yml",
        ],
        "the set of chain-carrying accepted documents changed"
    );
}

/// The other half of clause 3, and the reason the rule is safe to add under
/// the 1.x freeze: the shipped example set stays clean. Asserted here over the
/// real binary as well as by `tests/lint_fallback_asymmetry.rs`, which must
/// stay green unmodified.
#[test]
fn the_new_rule_is_silent_on_the_shipped_examples() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/infraportal/slos");
    let specs: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("examples/infraportal/slos exists")
        .map(|e| e.expect("readable dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "yaml"))
        .collect();
    assert!(
        !specs.is_empty(),
        "zero specs discovered in {}; the clean-set claim would be vacuous",
        dir.display()
    );
    for spec in &specs {
        let text = combined(&slokit(&["lint", "-i", &spec.to_string_lossy()]));
        assert!(
            !text.contains("SLO_PLUGIN_CHAIN_DROPPED"),
            "{} is a slokit-native spec with no plugin chain, but the rule fired:\n{text}",
            spec.display()
        );
    }
}
