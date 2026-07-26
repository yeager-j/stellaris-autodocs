//! Throwaway probe: show both adapters' item lists for one construct.
//!
//! ```text
//! cargo run --manifest-path tools/parser-spike/Cargo.toml --example inspect -- '<source>'
//! ```

use parser_spike::model::{Item, ParsedFile, Value};
use parser_spike::{lexer, tape};

fn outline(file: &ParsedFile) -> Vec<String> {
    file.items.iter().map(describe).collect()
}

fn describe(item: &Item) -> String {
    match item {
        Item::Field(field) => format!(
            "field {} {} {}",
            field.key.text(),
            field.operator.symbol(),
            shape(&field.value)
        ),
        Item::Element(value) => format!("element {}", shape(value)),
        Item::Conditional(conditional) => format!(
            "conditional [[{}{}]",
            if conditional.negated { "!" } else { "" },
            String::from_utf8_lossy(&conditional.parameter)
        ),
    }
}

fn shape(value: &Value) -> String {
    match value {
        Value::Scalar(scalar) => format!("`{}`", scalar.text()),
        Value::Container(container) => format!("{:?}({})", container.kind, container.items.len()),
        Value::Tagged { tag, .. } => format!("tagged:{}", tag.text()),
    }
}

fn main() {
    let argument = std::env::args().nth(1).unwrap_or_else(|| {
        "v = {\n\toptimize_memory\n\tif = { limit = { a = b } }\n\tx = 1\n}".to_string()
    });

    // A path means "parse this real corpus file and show me the divergence in context".
    let path = std::path::Path::new(&argument);
    if path.is_file() {
        let data = std::fs::read(path).expect("readable");
        let taped = tape::parse(&data).expect("tape parses");
        let lexed = lexer::parse(&data);
        let found = parser_spike::digest::first_divergence(&taped, &lexed)
            .expect("the file diverges");
        println!("{}: {}", found.path, found.detail);

        // Locate the divergent definition by its top-level index and print its source.
        if let Some(index) = found
            .path
            .strip_prefix('[')
            .and_then(|rest| rest.split(']').next())
            .and_then(|digits| digits.parse::<usize>().ok())
        {
            if let Some(Item::Field(field)) = lexed.items.get(index) {
                if let Some(span) = field.span {
                    let text = String::from_utf8_lossy(&data[span.range()]);
                    println!("--- source ({} bytes)\n{}", span.len(), &text[..text.len().min(1400)]);
                }
            }
        } else {
            println!("--- root-level divergence; tape items {} vs lexer items {}",
                taped.items.len(), lexed.items.len());
            for (i, (a, b)) in taped.items.iter().zip(&lexed.items).enumerate() {
                if describe(a) != describe(b) {
                    let from = i.saturating_sub(4);
                    for j in from..(i + 3).min(taped.items.len()).min(lexed.items.len()) {
                        println!(
                            "  {j:>4} tape {:<44} | lexer {}",
                            describe(&taped.items[j]),
                            describe(&lexed.items[j])
                        );
                    }
                    break;
                }
            }
        }
        return;
    }

    let source = argument;
    let data = source.as_bytes();

    match tape::parse(data) {
        Ok(file) => {
            println!("tape:");
            for line in outline(&file) {
                println!("  {line}");
            }
            if let Some(Item::Field(field)) = file.items.first() {
                if let Value::Container(container) = &field.value {
                    println!("  inner: {:?}", container.items.iter().map(describe).collect::<Vec<_>>());
                }
            }
        }
        Err(error) => println!("tape: ERR {}", error.message),
    }

    let file = lexer::parse(data);
    println!("lexer:");
    for line in outline(&file) {
        println!("  {line}");
    }
    if let Some(Item::Field(field)) = file.items.first() {
        if let Value::Container(container) = &field.value {
            println!("  inner: {:?}", container.items.iter().map(describe).collect::<Vec<_>>());
        }
    }
}
