//! Throwaway probe: how do the two Jomini APIs handle Stellaris's parameter syntax?
//!
//! `[[NAME] body ]` is conditional compilation on an inline-script argument. Jomini
//! documents it as EU4 syntax, but vanilla Stellaris uses it in `common/script_values` and
//! `common/scripted_effects`, and Gigastructural Engineering uses forms beyond it. The
//! coverage run failed on those files, so this establishes which forms each API accepts
//! before deciding what the wrapper has to do.

use jomini::text::{TextTape, Token, TokenReader};

fn probe(label: &str, source: &str) {
    println!("--- {label}\n    {source:?}");

    match TextTape::from_slice(source.as_bytes()) {
        Ok(tape) => println!("  tape:  OK {:?}", tape.tokens()),
        Err(error) => println!("  tape:  ERR {error}"),
    }

    let mut reader = TokenReader::from_slice(source.as_bytes());
    let mut rendered = Vec::new();
    loop {
        match reader.next() {
            Ok(Some(token)) => rendered.push(match token {
                Token::Open => "{".to_string(),
                Token::Close => "}".to_string(),
                Token::Operator(operator) => operator.symbol().to_string(),
                Token::Unquoted(scalar) => {
                    format!("u:{}", String::from_utf8_lossy(scalar.as_bytes()))
                }
                Token::Quoted(scalar) => {
                    format!("q:{}", String::from_utf8_lossy(scalar.as_bytes()))
                }
            }),
            Ok(None) => break,
            Err(error) => {
                rendered.push(format!("ERR({:?}@{})", error.kind(), error.position()));
                break;
            }
        }
    }
    println!("  lexer: {rendered:?}");
}

fn main() {
    probe("plain", "v = { base = 1 }");
    probe(
        "parameter block",
        "v = {\n\tbase = 1\n\t[[EXTRA]\n\t\tadd = $EXTRA$\n\t]\n\tmult = 2\n}",
    );
    probe("inline parameter block", "v = {\n\t[[A] base = 1 ]\n}");
    probe("negated parameter", "v = {\n\t[[!A] base = 1 ]\n}");
    probe("bare double bracket", "v = {\n\t[[FACTOR]]\n\tbase = 1\n}");
    probe("escaped expression", "v = {\n\tx = @\\[ 1 + 2 ]\n}");
    probe("absolute value expression", "v = {\n\tx = @\\[ |1 - 2| ]\n}");
    probe("bare token list", "alpha\nbeta\ngamma\n");
    probe("header value", "c = rgb { 1 2 3 }");
}
