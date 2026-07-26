//! Throwaway probe: raw tape tokens for a source string, or around a needle in a file.
//!
//! ```text
//! cargo run --example rawtape -- '<source>'
//! cargo run --example rawtape -- <path> <needle>
//! ```
use jomini::text::TextTape;

fn main() {
    let first = std::env::args().nth(1).expect("source or path argument");
    let needle = std::env::args().nth(2);

    let owned;
    let data: &[u8] = match (&needle, std::path::Path::new(&first).is_file()) {
        (Some(_), true) => {
            owned = std::fs::read(&first).expect("readable");
            &owned
        }
        _ => first.as_bytes(),
    };

    let tape = match TextTape::from_slice(data) {
        Ok(tape) => tape,
        Err(error) => {
            println!("ERR {error}");
            return;
        }
    };
    let tokens = tape.tokens();

    let mixed: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| matches!(token, jomini::text::TextToken::MixedContainer))
        .map(|(index, _)| index)
        .collect();
    println!("MixedContainer at {:?}", &mixed[..mixed.len().min(10)]);

    let focus = needle.as_ref().and_then(|needle| {
        tokens.iter().position(|token| {
            token
                .as_scalar()
                .is_some_and(|scalar| scalar.as_bytes() == needle.as_bytes())
        })
    });

    let (from, to) = match focus {
        Some(index) => (index.saturating_sub(12), (index + 6).min(tokens.len())),
        None => (0, tokens.len().min(40)),
    };
    for (index, token) in tokens.iter().enumerate().take(to).skip(from) {
        println!("{index:>6} {token:?}");
    }
    println!("(total {} tokens)", tokens.len());
}
