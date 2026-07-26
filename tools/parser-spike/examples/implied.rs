//! Throwaway probe: when is `scalar {` an implied assignment, and when is it a header?
//!
//! Vanilla writes both. `common/named_colors/01_trait_colors.txt` ends with
//! `trait_bg_active_glow { color = … }` — a definition with no `=` — while
//! `color = rgb { 1 2 3 }` is a tagged literal. The two look identical to a lexer, so the
//! rule has to come from the tape rather than from a guess.

use jomini::text::TextTape;

fn probe(label: &str, source: &str) {
    println!("--- {label}: {source:?}");
    match TextTape::from_slice(source.as_bytes()) {
        Ok(tape) => println!("  {:?}", tape.tokens()),
        Err(error) => println!("  ERR {error}"),
    }
}

fn main() {
    probe("top-level implied", "a { b = 1 }");
    probe("nested implied", "x = { a { b = 1 } }");
    probe("implied in an array-looking container", "x = { a { b } }");
    probe("header in value position", "x = rgb { 1 2 3 }");
    probe("header inside an array", "x = { rgb { 1 2 3 } rgb { 4 5 6 } }");
    probe("scalar then container in an array", "x = { alpha { 1 2 } }");
    probe("locator shape", "e = { locator = { name = root rotation {0 0 0}} }");
}
