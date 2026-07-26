//! Behavioural tests for both adapters over small, readable inputs.
//!
//! These establish that the two paths agree and that the checks used across the corpus can
//! actually fail. A gate that has only ever been green proves nothing
//! (`AGENTS.md`, Diagnostics), so `span_check_detects_a_wrong_span` deliberately corrupts a
//! span and requires the checker to notice.

use parser_spike::digest::{digest, first_divergence};
use parser_spike::model::{ContainerKind, Item, Operator, ParsedFile, ScalarKind, Value};
use parser_spike::{lexer, tape};

fn both(source: &str) -> (ParsedFile, ParsedFile) {
    let data = source.as_bytes();
    let a = tape::parse(data).expect("tape adapter parses the fixture");
    let b = lexer::parse(data);
    assert!(
        b.faults.is_empty(),
        "lexer adapter reported faults on valid source: {:?}",
        b.faults
    );
    (a, b)
}

fn agree(source: &str) -> ParsedFile {
    let (a, b) = both(source);
    if let Some(divergence) = first_divergence(&a, &b) {
        panic!(
            "adapters disagree at {}: {}\nsource: {source}",
            divergence.path, divergence.detail
        );
    }
    assert_eq!(digest(&a), digest(&b));

    let faults = lexer::verify_spans(source.as_bytes(), &b);
    assert!(faults.is_empty(), "span round-trip failed: {faults:?}");
    b
}

fn field<'a>(file: &'a ParsedFile, index: usize) -> &'a parser_spike::model::Field {
    match &file.items[index] {
        Item::Field(field) => field,
        other => panic!("item {index} is not a field: {other:?}"),
    }
}

#[test]
fn duplicate_keys_survive_in_order() {
    // Duplicate resolution runs in opposite directions between registries, so the parser
    // must hand the resolver both definitions and their order, never a merged map
    // (docs/spikes/resolver-evaluation.md:113).
    let file = agree("a = 1\na = 2\na = 3\n");
    assert_eq!(file.definitions().count(), 3);
    let values: Vec<_> = file
        .definitions()
        .map(|field| match &field.value {
            Value::Scalar(scalar) => scalar.text().into_owned(),
            other => panic!("expected scalar, got {other:?}"),
        })
        .collect();
    assert_eq!(values, ["1", "2", "3"]);
}

#[test]
fn every_operator_survives() {
    let file = agree(
        "a = 1\nb == 2\nc != 3\nd < 4\ne <= 5\nf > 6\ng >= 7\nh ?= 8\n",
    );
    let operators: Vec<_> = file.definitions().map(|field| field.operator).collect();
    assert_eq!(
        operators,
        [
            Operator::Equal,
            Operator::Exact,
            Operator::NotEqual,
            Operator::LessThan,
            Operator::LessThanEqual,
            Operator::GreaterThan,
            Operator::GreaterThanEqual,
            Operator::Exists,
        ]
    );
}

#[test]
fn container_kinds_are_derived_from_content() {
    let file = agree("empty = { }\nobject = { a = 1 }\narray = { x y }\nmixed = { a = 1 x }\n");
    let kinds: Vec<_> = file
        .definitions()
        .map(|field| match &field.value {
            Value::Container(container) => container.kind,
            other => panic!("expected container, got {other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            ContainerKind::Empty,
            ContainerKind::Object,
            ContainerKind::Array,
            ContainerKind::Mixed
        ]
    );
}

#[test]
fn numeric_lexemes_are_preserved_exactly() {
    // D-099 and technical-design.md:328 forbid binary floating point from reaching source
    // equality or displayed Base Values, so the parser must not normalize a lexeme.
    let file = agree("a = 0.10\nb = 1.0\nc = 1\nd = -2.50\ne = 2200.1.1\n");
    let scalars: Vec<_> = file
        .definitions()
        .map(|field| match &field.value {
            Value::Scalar(scalar) => (scalar.kind, scalar.text().into_owned()),
            other => panic!("expected scalar, got {other:?}"),
        })
        .collect();

    assert_eq!(scalars[0], (ScalarKind::Number, "0.10".to_owned()));
    assert_eq!(scalars[1], (ScalarKind::Number, "1.0".to_owned()));
    assert_eq!(scalars[2], (ScalarKind::Number, "1".to_owned()));
    assert_eq!(scalars[3], (ScalarKind::Number, "-2.50".to_owned()));
    // A date is not a number. Classifying it as one would invent a value.
    assert_eq!(scalars[4], (ScalarKind::Unquoted, "2200.1.1".to_owned()));
}

#[test]
fn unresolved_variable_references_stay_references() {
    // The game reinterprets a field whose `@name` does not resolve as a script-value block
    // and swallows the following lines (docs/spikes/resolver-evaluation.md:244). The parser
    // must not imitate that, or the resolver can never detect the condition.
    let file = agree("@oracle_fwd = @oracle_fwd_target\ncost = @oracle_fwd\ntier = 1\n");
    assert_eq!(file.definitions().count(), 3);

    let declaration = field(&file, 0);
    assert_eq!(declaration.key.kind, ScalarKind::VariableRef);
    match &declaration.value {
        Value::Scalar(scalar) => assert_eq!(scalar.kind, ScalarKind::VariableRef),
        other => panic!("a constant-valued constant is not a block: {other:?}"),
    }

    let consumer = field(&file, 1);
    match &consumer.value {
        Value::Scalar(scalar) => {
            assert_eq!(scalar.kind, ScalarKind::VariableRef);
            assert_eq!(scalar.text(), "@oracle_fwd");
        }
        other => panic!("`cost = @oracle_fwd` must stay a reference, got {other:?}"),
    }
    // The definition after the unresolved reference is still visible, which is exactly what
    // the game loses.
    assert_eq!(field(&file, 2).key.text(), "tier");
}

#[test]
fn inline_script_forms_are_preserved_not_expanded() {
    let file = agree(
        "weight_modifier = {\n\tinline_script = \"oracle/factor_block\"\n}\n\
         other = {\n\tinline_script = {\n\t\tscript = oracle/param_factor\n\t\tF = 1000000\n\t}\n}\n",
    );

    let scalar_form = match &field(&file, 0).value {
        Value::Container(container) => container.fields().next().unwrap(),
        other => panic!("expected container, got {other:?}"),
    };
    assert_eq!(scalar_form.key.text(), "inline_script");
    match &scalar_form.value {
        Value::Scalar(scalar) => {
            assert_eq!(scalar.kind, ScalarKind::Quoted);
            assert_eq!(scalar.text(), "oracle/factor_block");
        }
        other => panic!("scalar form must stay a scalar, got {other:?}"),
    }

    let block_form = match &field(&file, 1).value {
        Value::Container(container) => container.fields().next().unwrap(),
        other => panic!("expected container, got {other:?}"),
    };
    match &block_form.value {
        Value::Container(container) => {
            let keys: Vec<_> = container
                .fields()
                .map(|field| field.key.text().into_owned())
                .collect();
            assert_eq!(keys, ["script", "F"]);
        }
        other => panic!("parameterized form must stay a block, got {other:?}"),
    }
}

#[test]
fn inline_script_fragments_parse_as_files() {
    // An inline-script callee is a fragment, not a whole definition. If the parser required
    // a complete definition per file it would reject every one of them.
    let file = agree("modifier = {\n\tfactor = $F$\n\talways = yes\n}\n");
    let modifier = match &field(&file, 0).value {
        Value::Container(container) => container,
        other => panic!("expected container, got {other:?}"),
    };
    let factor = modifier.fields().next().unwrap();
    match &factor.value {
        Value::Scalar(scalar) => assert_eq!(scalar.kind, ScalarKind::Parameter),
        other => panic!("`$F$` must survive as a parameter, got {other:?}"),
    }
}

#[test]
fn inner_field_keys_remain_addressable() {
    // Ship components are keyed by an inner `key` field, not by the block name; treating
    // the block name as the key would collapse every utility component into one entry
    // (docs/spikes/resolver-evaluation.md:291).
    let file = agree(
        "utility_component_template = {\n\tkey = \"SMALL_ARMOR_1\"\n\tsize = small\n}\n\
         utility_component_template = {\n\tkey = \"SMALL_ARMOR_2\"\n\tsize = small\n}\n",
    );
    assert_eq!(file.definitions().count(), 2);

    let inner: Vec<_> = file
        .definitions()
        .map(|field| {
            let Value::Container(container) = &field.value else {
                panic!("expected container");
            };
            let key = container
                .fields()
                .find(|inner| inner.key.text() == "key")
                .expect("component defines an inner key");
            match &key.value {
                Value::Scalar(scalar) => scalar.text().into_owned(),
                other => panic!("expected scalar key, got {other:?}"),
            }
        })
        .collect();

    assert_eq!(inner, ["SMALL_ARMOR_1", "SMALL_ARMOR_2"]);
    // Both blocks share an enclosing name, which is why it cannot be the identity.
    for definition in file.definitions() {
        assert_eq!(definition.key.text(), "utility_component_template");
    }
}

#[test]
fn comments_do_not_become_content() {
    let file = agree("# leading comment\na = 1 # trailing comment\n# tail\n");
    assert_eq!(file.definitions().count(), 1);
    assert_eq!(field(&file, 0).key.text(), "a");
}

#[test]
fn descriptor_lexical_style_parses() {
    // descriptor.mod uses `key="value"` with no spaces and `tags={`, a different lexical
    // style from tab-indented script files.
    let file = agree(
        "name=\"Stellaris Docs Oracle Target\"\nversion=\"1.0\"\nreplace_path=\"common/technology\"\ntags={\n\t\"Utilities\"\n}\n",
    );
    assert_eq!(field(&file, 0).key.text(), "name");
    assert_eq!(field(&file, 2).key.text(), "replace_path");
    match &field(&file, 3).value {
        Value::Container(container) => assert_eq!(container.kind, ContainerKind::Array),
        other => panic!("expected array, got {other:?}"),
    }
}

#[test]
fn spans_locate_whole_definitions() {
    // Evidence requirement 3 asks for ranges that can show a bounded excerpt of a top-level
    // definition, which needs the braces the tape cannot locate.
    let source = "tier = 1\ntech_x = {\n\tcost = 100\n}\n";
    let file = lexer::parse(source.as_bytes());
    let definition = field(&file, 1);
    let span = definition.span.expect("lexer supplies definition spans");
    assert_eq!(&source[span.range()], "tech_x = {\n\tcost = 100\n}");
}

#[test]
fn span_check_detects_a_wrong_span() {
    // Negative control. verify_spans is the gate that certifies every range in the corpus
    // run, so it has to be shown going red before its green result means anything.
    let source = "tech_x = { cost = 100 }";
    let mut file = lexer::parse(source.as_bytes());
    assert!(lexer::verify_spans(source.as_bytes(), &file).is_empty());

    match &mut file.items[0] {
        Item::Field(field) => {
            let span = field.key.span.as_mut().expect("key has a span");
            span.start += 1;
        }
        other => panic!("expected a field, got {other:?}"),
    }

    let faults = lexer::verify_spans(source.as_bytes(), &file);
    assert_eq!(faults.len(), 1, "a shifted span must be reported");
    assert_eq!(faults[0].expected, "tech_x");
    assert_eq!(faults[0].found, "ech_x");
}

const UNCLOSED: &str = "\
first = { a = 1 }
broken = { a = 1
second = { b = 2 }
third = { c = 3 }
";

fn names(file: &ParsedFile) -> Vec<String> {
    file.definitions()
        .map(|field| field.key.text().into_owned())
        .collect()
}

#[test]
fn the_tape_accepts_an_unclosed_brace_and_silently_reshapes_the_file() {
    // Not the failure mode the spike anticipated. An unbalanced open brace is not rejected:
    // the tape succeeds, and everything after the brace becomes a child of the definition
    // that failed to close. `second` and `third` are still present in the tree, at the
    // wrong depth, under the wrong parent, with nothing reported.
    //
    // A silent reshape is worse for this application than a refusal, because there is no
    // signal to attach an Analysis Issue to (technical-design.md:332). It is recorded here
    // so the behaviour is pinned rather than rediscovered.
    let file = tape::parse(UNCLOSED.as_bytes()).expect("the tape accepts an unclosed brace");
    assert_eq!(names(&file), ["first", "broken"]);

    let Value::Container(broken) = &field(&file, 1).value else {
        panic!("expected a container");
    };
    let swallowed: Vec<_> = broken
        .fields()
        .map(|inner| inner.key.text().into_owned())
        .collect();
    assert_eq!(swallowed, ["a", "second", "third"]);
}

#[test]
fn the_lexer_reports_an_unclosed_brace_and_recovers_the_rest() {
    let file = lexer::parse(UNCLOSED.as_bytes());

    assert_eq!(names(&file), ["first", "second", "third"]);
    assert_eq!(file.faults.len(), 1);

    let fault = &file.faults[0];
    // The fault is attributed to the brace that never closed, not to the end of file where
    // the reader noticed.
    assert_eq!(&UNCLOSED[fault.offset as usize..fault.offset as usize + 1], "{");
    assert!(fault.resumed_at.is_some());
    // `broken` cost its own definition and the three items nested under it. Recovery does
    // not hoist those into the parent, because reporting them at top level would state
    // structure the source does not contain.
    assert_eq!(fault.abandoned, 3);
}

#[test]
fn an_unterminated_quote_costs_one_definition_rather_than_the_file() {
    let source = "\
first = { a = 1 }
broken = { a = \"oops }
second = { b = 2 }
third = { c = 3 }
";
    assert!(
        tape::parse(source.as_bytes()).is_err(),
        "the tape rejects this file, so its whole content is unavailable"
    );

    let file = lexer::parse(source.as_bytes());
    assert_eq!(names(&file), ["first", "second", "third"]);
    assert_eq!(file.faults.len(), 1);
}
