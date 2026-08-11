//! Guards for the crate's PUBLISHED examples: the `README.md` that crates.io
//! renders as the front page, and the rustdoc examples docs.rs renders as the
//! landing page.
//!
//! Until this suite there was no test of either. `README.md` was read by
//! nothing in the repo, and `src/lib.rs`'s "Generating Prometheus rules"
//! example was fenced ```` ```ignore ````, which means rustdoc never compiled
//! it. It had rotted accordingly: on the tree this suite was written against
//! it called `Spec::from_yaml(yaml_str)` with `yaml_str` undefined, used `?`
//! in a `()` body, and rendered `ruleset.to_yaml()` -- a method `RuleSet` has
//! never had. Un-ignoring it produced four compile errors, the last being
//! `error[E0599]: no method named `to_yaml` found for struct `RuleSet``. That
//! example had shipped on docs.rs in every published release.
//!
//! The two guards below make that class impossible rather than fixing one
//! instance of it:
//!
//! 1. No rustdoc example anywhere under `src/` may be marked `ignore`. An
//!    ignored example is an UNCOMPILED example, so it is documentation that
//!    no gate reads. `no_run` stays allowed: it is compiled, only not run,
//!    which is what an example that opens a file off disk needs.
//! 2. Every ```` ```rust ```` block in `README.md` must appear as the VISIBLE
//!    body of a compiled rustdoc example under `src/`. That is what proves
//!    the README's Rust compiles without including the README itself as a
//!    doctest: its twin is compiled by `cargo test --all-features`, and the
//!    two can no longer drift (they already had -- one comment line differed).
//!
//! Plus two smaller contracts on the same surface: a README code fence may
//! not carry a rustdoc hidden-line marker (`README.md` is rendered as plain
//! markdown, so `# ...` is shown literally and is a syntax error if pasted),
//! and the dependency pin the README hands library consumers must name a
//! version of this crate that exists (it said `0.12` while the crate was at
//! 1.9.0 -- a requirement resolving to the pre-1.0 line, on the page
//! crates.io shows first).
//!
//! The `src/` file list is discovered by walking the tree, never
//! hand-enumerated, and an empty discovery is a hard failure.

use std::fs;
use std::path::{Path, PathBuf};

/// A fenced code block lifted out of a document.
#[derive(Debug)]
struct Block {
    /// Repo-relative path of the file the block came from.
    source: String,
    /// 1-based line number of the opening fence.
    line: usize,
    /// The fence info string, trimmed (`rust`, `no_run`, `ignore`, `text`, ...).
    info: String,
    /// The block body, one entry per line, without the fences.
    body: Vec<String>,
}

impl Block {
    /// The lines a reader actually sees: rustdoc hidden lines removed.
    fn visible(&self) -> Vec<String> {
        self.body
            .iter()
            .filter(|l| !is_hidden_doctest_line(l))
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    fn where_(&self) -> String {
        format!("{}:{}", self.source, self.line)
    }
}

/// rustdoc hides a doctest line that starts with `# ` (or is exactly `#`).
fn is_hidden_doctest_line(line: &str) -> bool {
    let t = line.trim_start();
    t == "#" || t.starts_with("# ")
}

/// Would rustdoc COMPILE a block with this info string?
///
/// `ignore` and any non-Rust language (`text`, `yaml`, `sh`, ...) are not
/// compiled. `compile_fail` is deliberately excluded too: a block that is
/// expected not to compile is no evidence that an example works.
fn is_compiled_rust(info: &str) -> bool {
    let tokens: Vec<&str> = info
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        // A bare ``` fence inside a rustdoc comment is Rust.
        return true;
    }
    const COMPILED: &[&str] = &[
        "rust",
        "no_run",
        "should_panic",
        "edition2015",
        "edition2018",
        "edition2021",
        "edition2024",
    ];
    tokens.iter().all(|t| COMPILED.contains(t))
}

fn manifest_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// Read a repo file with CRLF normalised away: this repo's working tree is
/// CRLF on Windows and LF on the runner, and neither may change a verdict.
fn read_normalised(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// Every `.rs` file under `src/`, discovered by walking the tree. Empty
/// discovery is a hard failure: a guard that silently scans nothing proves
/// nothing.
fn src_files() -> Vec<PathBuf> {
    let root = manifest_path("src");
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("cannot read source dir {}: {e}", dir.display()))
        {
            let path = entry.expect("readable dir entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    assert!(
        !found.is_empty(),
        "discovered no .rs files under {} -- this guard would pass vacuously",
        root.display()
    );
    found.sort();
    let lib = manifest_path("src/lib.rs");
    assert!(
        found.contains(&lib),
        "the source walk did not reach src/lib.rs, so it is not walking the crate: {found:?}"
    );
    found
}

fn rel(path: &Path) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.strip_prefix(&root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Fenced blocks in a plain markdown document.
fn markdown_blocks(source: &str, text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut open: Option<Block> = None;
    for (idx, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("```") {
            match open.take() {
                Some(block) => blocks.push(block),
                None => {
                    open = Some(Block {
                        source: source.to_string(),
                        line: idx + 1,
                        info: rest.trim().to_string(),
                        body: Vec::new(),
                    })
                }
            }
        } else if let Some(block) = open.as_mut() {
            block.body.push(line.to_string());
        }
    }
    assert!(
        open.is_none(),
        "{source} ends with an unclosed code fence opened at line {}",
        open.map(|b| b.line).unwrap_or(0)
    );
    blocks
}

/// Fenced blocks inside the rustdoc comments (`//!` and `///`) of a Rust file.
fn rustdoc_blocks(source: &str, text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut open: Option<Block> = None;
    for (idx, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        let content = trimmed
            .strip_prefix("//!")
            .or_else(|| trimmed.strip_prefix("///"));
        let Some(content) = content else {
            // A non-doc line ends any doc block; an unterminated fence inside
            // one is malformed documentation rather than a block to compare.
            open = None;
            continue;
        };
        let content = content.strip_prefix(' ').unwrap_or(content);
        if let Some(rest) = content.trim_start().strip_prefix("```") {
            match open.take() {
                Some(block) => blocks.push(block),
                None => {
                    open = Some(Block {
                        source: source.to_string(),
                        line: idx + 1,
                        info: rest.trim().to_string(),
                        body: Vec::new(),
                    })
                }
            }
        } else if let Some(block) = open.as_mut() {
            block.body.push(content.to_string());
        }
    }
    blocks
}

fn all_rustdoc_blocks() -> Vec<Block> {
    let mut blocks = Vec::new();
    for path in src_files() {
        let source = rel(&path);
        blocks.extend(rustdoc_blocks(&source, &read_normalised(&path)));
    }
    blocks
}

fn readme() -> String {
    read_normalised(&manifest_path("README.md"))
}

/// The README's Rust examples. Empty discovery is a hard failure.
fn readme_rust_blocks() -> Vec<Block> {
    let text = readme();
    let blocks: Vec<Block> = markdown_blocks("README.md", &text)
        .into_iter()
        .filter(|b| b.info == "rust")
        .collect();
    assert!(
        !blocks.is_empty(),
        "README.md has no ```rust blocks -- either the README lost its library \
         examples or this extractor stopped matching, and both make the \
         agreement guard below vacuous"
    );
    blocks
}

// ---------------------------------------------------------------------------

/// An `ignore`d rustdoc example is never compiled, so nothing reports it when
/// the API it demonstrates changes underneath it. `no_run` is the supported
/// way to document something that must not execute during a test run.
#[test]
fn no_rustdoc_example_in_src_is_marked_ignore() {
    let ignored: Vec<String> = all_rustdoc_blocks()
        .iter()
        .filter(|b| {
            b.info
                .split(|c: char| c == ',' || c.is_whitespace())
                .any(|t| t == "ignore")
        })
        .map(|b| format!("{} (```{})", b.where_(), b.info))
        .collect();
    assert!(
        ignored.is_empty(),
        "these rustdoc examples are marked `ignore`, so rustdoc never compiles \
         them and they are documentation no gate reads: {ignored:?}. Use \
         `no_run` if the example must not execute -- that still compiles it."
    );
}

/// Every Rust example the README shows must be the visible body of a compiled
/// rustdoc example under `src/`. The README itself is not a doctest, so this
/// agreement is the only thing that makes its Rust provably compile.
#[test]
fn every_readme_rust_example_is_a_compiled_doctest_in_src() {
    let compiled: Vec<Block> = all_rustdoc_blocks()
        .into_iter()
        .filter(|b| is_compiled_rust(&b.info))
        .collect();
    assert!(
        !compiled.is_empty(),
        "no compiled rustdoc examples were found under src/ at all"
    );

    for block in readme_rust_blocks() {
        let want = block.visible();
        let matched = compiled.iter().any(|c| c.visible() == want);
        assert!(
            matched,
            "the README's Rust example at {} is not the visible body of any \
             compiled rustdoc example under src/, so nothing compiles it.\n\
             --- README block ---\n{}\n--- compiled rustdoc examples found ---\n{}",
            block.where_(),
            want.join("\n"),
            compiled
                .iter()
                .map(|c| format!("{} (```{})\n{}", c.where_(), c.info, c.visible().join("\n")))
                .collect::<Vec<_>>()
                .join("\n---\n")
        );
    }
}

/// `README.md` is rendered as plain markdown by GitHub and crates.io, so a
/// rustdoc hidden-line marker is not hidden there: it is displayed verbatim,
/// and `# Ok::<(), slokit::SlokitError>(())` is a syntax error if pasted.
#[test]
fn no_readme_rust_block_carries_a_rustdoc_hidden_line_marker() {
    for block in readme_rust_blocks() {
        let leaked: Vec<&String> = block
            .body
            .iter()
            .filter(|l| is_hidden_doctest_line(l))
            .collect();
        assert!(
            leaked.is_empty(),
            "the README ```rust block at {} carries rustdoc hidden-line \
             markers {leaked:?}. README.md is plain markdown, so those lines \
             are shown to the reader instead of hidden, and they do not \
             compile if pasted. Keep the hidden scaffolding in the src/ \
             doctest only.",
            block.where_()
        );
    }
}

/// The README tells library consumers which version to depend on. It named
/// `0.12` while this crate was at 1.9.0 -- a requirement that resolves to the
/// pre-1.0 line with a different API, on the page crates.io renders first.
#[test]
fn the_readme_dependency_pin_names_a_version_of_this_crate() {
    let text = readme();
    let current = env!("CARGO_PKG_VERSION");
    let current_parts = numeric_parts(current);
    assert_eq!(
        current_parts.len(),
        3,
        "CARGO_PKG_VERSION {current} is not three numeric components"
    );

    let mut pins = Vec::new();
    let needle = "slokit = { version = \"";
    let mut rest = text.as_str();
    while let Some(at) = rest.find(needle) {
        let after = &rest[at + needle.len()..];
        let end = after
            .find('"')
            .unwrap_or_else(|| panic!("unterminated version string in README near {needle}"));
        pins.push(after[..end].to_string());
        rest = &after[end..];
    }
    assert!(
        !pins.is_empty(),
        "README.md names no `slokit = {{ version = \"...\" }}` dependency pin. \
         Either the install guidance was dropped or this extractor stopped \
         matching it; both leave the pin unguarded."
    );

    for pin in &pins {
        let parts = numeric_parts(pin);
        assert!(
            !parts.is_empty(),
            "README dependency pin {pin:?} has no numeric version components"
        );
        // Same major line, and not a release that does not exist yet. The
        // slice comparison is lexicographic, which is the ordering semver
        // uses for the numeric components.
        let ok = parts.len() <= current_parts.len()
            && parts[0] == current_parts[0]
            && parts.as_slice() <= &current_parts[..parts.len()];
        assert!(
            ok,
            "README.md tells consumers `slokit = {{ version = \"{pin}\" }}` but \
             this crate is {current}. A pin whose major differs (or that names \
             a release that does not exist yet) resolves to a different API \
             than the one the README documents."
        );
    }
}

/// Leading numeric components of a version or requirement string.
fn numeric_parts(v: &str) -> Vec<u64> {
    v.trim_start_matches(['^', '~', '=', ' '])
        .split(['.', '-', '+'])
        .map_while(|p| p.parse::<u64>().ok())
        .collect()
}
