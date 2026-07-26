//! Throwaway probe: which syntax faults does `TextTape::from_slice` actually reject?
//!
//! The blast-radius measurement assumes a malformed file fails as a unit. Whether a given
//! fault fails at all has to be established before that assumption means anything.

use parser_spike::{lexer, tape};

fn probe(label: &str, source: &str) {
    println!("--- {label}");
    match tape::parse(source.as_bytes()) {
        Ok(file) => {
            let names: Vec<_> = file
                .definitions()
                .map(|field| field.key.text().into_owned())
                .collect();
            println!("  tape:  OK, {} top-level definitions {names:?}", names.len());
        }
        Err(error) => println!("  tape:  ERR {} @{:?}", error.message, error.offset),
    }

    let file = lexer::parse(source.as_bytes());
    let names: Vec<_> = file
        .definitions()
        .map(|field| field.key.text().into_owned())
        .collect();
    println!(
        "  lexer: {} definitions {names:?}, faults {:?}",
        names.len(),
        file.faults
    );
}

fn main() {
    let clean = "first = { a = 1 }\nsecond = { b = 2 }\nthird = { c = 3 }\n";
    probe("clean", clean);
    probe(
        "unclosed brace",
        "first = { a = 1 }\nbroken = { a = 1\nsecond = { b = 2 }\nthird = { c = 3 }\n",
    );
    probe(
        "stray close",
        "first = { a = 1 }\nbroken = { a = 1 } }\nsecond = { b = 2 }\nthird = { c = 3 }\n",
    );
    probe(
        "unterminated quote",
        "first = { a = 1 }\nbroken = { a = \"oops }\nsecond = { b = 2 }\nthird = { c = 3 }\n",
    );
    probe(
        "truncated",
        "first = { a = 1 }\nsecond = { b = 2 }\nthird = { c =",
    );
}
