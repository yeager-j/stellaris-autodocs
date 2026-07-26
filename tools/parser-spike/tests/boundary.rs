//! Evidence requirement 6: the application-owned representation does not expose Jomini
//! types beyond the parser boundary.
//!
//! `docs/technical-design.md:267` states the rule and `:186` extends it to every adapter.
//! Intent is not evidence, so this reads the sources and checks. It is a crude gate — a
//! substring search over text — but it is the earliest reliable one available at spike
//! scale, which is what `AGENTS.md` asks for: enforce a rule with the earliest reliable,
//! proportionate mechanism.
//!
//! The negative control is `tape.rs` and `lexer.rs`, which must mention Jomini. If the
//! search stopped matching there, it would have stopped working, and a green result on
//! `model.rs` would mean nothing.

use std::path::{Path, PathBuf};

fn source(name: &str) -> (PathBuf, String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    (path, text)
}

/// Modules that define or consume the model without being an adapter.
const PARSER_INDEPENDENT: &[&str] = &["model.rs", "digest.rs", "survey.rs", "classify.rs"];

/// Modules that are adapters, and must name the library they adapt.
const ADAPTERS: &[&str] = &["tape.rs", "lexer.rs"];

#[test]
fn the_model_and_its_consumers_never_name_the_parser_library() {
    for name in PARSER_INDEPENDENT {
        let (path, text) = source(name);
        for (number, line) in text.lines().enumerate() {
            // Prose may discuss Jomini; code may not depend on it.
            let is_comment = line.trim_start().starts_with("//");
            assert!(
                is_comment || !line.to_lowercase().contains("jomini"),
                "{}:{} names the parser library outside a comment: {line}",
                path.display(),
                number + 1
            );
        }
    }
}

#[test]
fn the_search_detects_a_dependency_where_one_exists() {
    // Negative control. Without this, a search that silently matched nothing would report
    // the boundary as clean forever.
    for name in ADAPTERS {
        let (path, text) = source(name);
        assert!(
            text.contains("use jomini::"),
            "{} is an adapter and must import the library the check looks for",
            path.display()
        );
    }
}
