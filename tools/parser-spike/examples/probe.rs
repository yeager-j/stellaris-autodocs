//! Throwaway probe: what does `TokenReader::position()` actually report, and when?
//!
//! Kept in the tree because the span derivation in `lexer.rs` rests on the answer, and a
//! future reader deserves the observation rather than the conclusion alone.

use jomini::text::{Token, TokenReader};

fn probe(label: &str, src: &str) {
    println!("--- {label}: {src:?}");
    let data = src.as_bytes();
    let mut reader = TokenReader::from_slice(data);
    loop {
        let before = reader.position();
        let rendered = {
            let token = match reader.next() {
                Ok(Some(token)) => token,
                Ok(None) => break,
                Err(error) => {
                    println!("  ERR {:?} @{}", error.kind(), error.position());
                    break;
                }
            };
            match token {
                Token::Open => "Open".to_string(),
                Token::Close => "Close".to_string(),
                Token::Operator(operator) => format!("Op({})", operator.symbol()),
                Token::Unquoted(scalar) => {
                    format!("Unq({})", String::from_utf8_lossy(scalar.as_bytes()))
                }
                Token::Quoted(scalar) => {
                    format!("Quo({})", String::from_utf8_lossy(scalar.as_bytes()))
                }
            }
        };
        println!("  before={before:>3} after={:>3}  {rendered}", reader.position());
    }
}

fn main() {
    probe("spaces", "a = 1\nbb = 2\n");
    probe("tight", "name=\"X\"\nv=2");
    probe("braces", "k = { a b }\n");
    probe("comment", "# c\na = 1 # t\n");
    probe("quoted", "a = \"x y\" b = 2");
    probe("bom", "\u{feff}a = 1");
    probe("ops", "a >= 1\nb != 2");
    probe("escape", "a = \"x\\\"y\" b = 2");
}
