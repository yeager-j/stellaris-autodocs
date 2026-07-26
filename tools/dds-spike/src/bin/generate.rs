//! Generate `fixtures/assets/dds/` from `src/fixtures.rs`, or check that it still matches.
//!
//! Binary fixtures cannot be reviewed by eye and cannot carry the header comment that
//! `fixtures/parser/malformed/*.txt` uses to state its prediction. So the bytes are never edited:
//! they are a pure function of a committed [`Fixture`](dds_spike::fixtures::Fixture), and
//! `--check` regenerates every file in memory and compares it to what is on disk. A fixture that
//! drifted from the source describing it fails `cargo test`.
//!
//! ```text
//! cargo run --manifest-path tools/dds-spike/Cargo.toml --bin generate            # check
//! cargo run --manifest-path tools/dds-spike/Cargo.toml --bin generate -- --write # rewrite
//! ```

use dds_spike::corpus;
use dds_spike::digest::sha256;
use dds_spike::fixtures;

fn main() -> std::io::Result<()> {
    let write = std::env::args().any(|argument| argument == "--write");
    let root = corpus::fixtures_root();
    let mut drifted = Vec::new();
    let mut written = 0usize;

    for fixture in fixtures::all() {
        let bytes = (fixture.bytes)();
        let path = root.join(fixture.path);
        if write {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &bytes)?;
            written += 1;
            println!(
                "{:44} {:5} bytes  {}",
                fixture.path,
                bytes.len(),
                &sha256(&bytes)[..16]
            );
        } else {
            match std::fs::read(&path) {
                Ok(existing) if existing == bytes => {}
                Ok(_) => drifted.push(format!("{}: on disk differs from the generator", fixture.path)),
                Err(error) => drifted.push(format!("{}: {error}", fixture.path)),
            }
        }
    }

    if write {
        println!("wrote {written} fixtures to {}", root.display());
        return Ok(());
    }

    if drifted.is_empty() {
        println!("all {} fixtures match their generator", fixtures::all().len());
        Ok(())
    } else {
        for line in &drifted {
            eprintln!("DRIFT {line}");
        }
        std::process::exit(1);
    }
}
