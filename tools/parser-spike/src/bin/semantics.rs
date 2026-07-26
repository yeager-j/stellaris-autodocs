//! `p6-semantics` — capture the fixture-driven checks for requirements 2, 5, and 8 to 11.
//!
//! The checks themselves live in `checks.rs` and also run under `cargo test`, so this
//! records what the test suite asserts rather than a second opinion about it.
//!
//! ```text
//! cargo run --release --manifest-path tools/parser-spike/Cargo.toml --bin semantics -- --capture
//! ```

use parser_spike::{checks, corpus, record};

const PURPOSE: &str = "Capture the semantic checks that fixtures answer and the corpus \
cannot. Coverage runs establish that files parse; these establish that what comes out means \
what the source said — a scripted constant still resolvable, a numeric lexeme unrounded, an \
inline script unexpanded, a conditional block still conditional, an inner-field key still \
addressable, and an unfamiliar byte still data. The same functions run under `cargo test`, \
so a green suite and a green record cannot disagree.";

fn main() -> std::io::Result<()> {
    let capture = std::env::args().any(|argument| argument == "--capture");

    let checks = checks::run();
    let mut lines = String::from("# outcome\trequirement\tcheck\tdetail\n");
    for check in &checks {
        println!("{}", check.line());
        lines.push_str(&check.line());
        lines.push('\n');
    }

    let failed = checks.iter().filter(|check| !check.passed()).count();
    println!("\n{} of {} checks passed", checks.len() - failed, checks.len());

    let warnings = if failed == 0 {
        Vec::new()
    } else {
        vec![format!(
            "{failed} semantic checks failed; no claim about requirements 8 to 11 may be \
             read from this record"
        )]
    };

    if !capture {
        eprintln!("(not captured; pass --capture to write the record)");
        return Ok(());
    }

    // Only the fixture corpus is pinned here: these checks read nothing else, and recording
    // the game corpora would imply they were inputs to a result they cannot affect.
    let fixtures = corpus::default_corpora()
        .into_iter()
        .find(|corpus| corpus.id == "fixtures")
        .expect("the fixture corpus is defined");
    let files = corpus::enumerate(&fixtures.root)?;
    let identities = vec![corpus::identify(&fixtures, &files)?];

    let directory = record::write(
        "p6-semantics",
        PURPOSE,
        identities,
        vec![("checks.txt".to_owned(), lines)],
        warnings,
    )?;
    eprintln!("captured {}", directory.display());

    if failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
