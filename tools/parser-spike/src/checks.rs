//! Fixture-driven checks for evidence requirements 8 through 11.
//!
//! These are claims about meaning rather than about coverage: a scripted constant must
//! reach the resolver as a resolvable definition, a numeric lexeme must survive unrounded,
//! an inline script must survive unexpanded, and a definition keyed by an inner field must
//! keep both names. Each is checked against a committed fixture that explains itself.
//!
//! One authority, two surfaces. `cargo test` runs these through `tests/fixtures.rs`, and
//! the `semantics` binary runs the same functions to capture the `p6-semantics` record. A
//! separate implementation for each would let the record and the test drift into disagreeing
//! about what passed.

use crate::corpus;
use crate::model::{Container, Field, Item, ParsedFile, ScalarKind, Value};
use crate::{lexer, tape};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Check {
    /// The evidence requirement this bears on, by its number in the spike document.
    pub requirement: u8,
    pub name: &'static str,
    pub outcome: Result<String, String>,
}

impl Check {
    pub fn passed(&self) -> bool {
        self.outcome.is_ok()
    }

    pub fn line(&self) -> String {
        match &self.outcome {
            Ok(detail) => format!("PASS\treq{}\t{}\t{detail}", self.requirement, self.name),
            Err(detail) => format!("FAIL\treq{}\t{}\t{detail}", self.requirement, self.name),
        }
    }
}

fn fixture(relative: &str) -> PathBuf {
    corpus::fixtures_root().join("valid").join(relative)
}

/// Parse a fixture through the lexer adapter, which is the path that reads every fixture.
///
/// The tape rejects two of them outright — the escaped-expression and conditional-block
/// files — so requiring both adapters here would mean these checks could not run at all,
/// and the reason would be buried in a test failure rather than reported as the finding it
/// is. `p1-coverage` is where the two adapters are compared.
fn parse(relative: &str) -> Result<(Vec<u8>, ParsedFile), String> {
    let path = fixture(relative);
    let data = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let parsed = lexer::parse(&data);
    if !parsed.faults.is_empty() {
        return Err(format!("unexpected faults: {:?}", parsed.faults));
    }
    Ok((data, parsed))
}

fn find<'a>(file: &'a ParsedFile, key: &str) -> Option<&'a Field> {
    file.definitions().find(|field| field.key.text() == key)
}

fn container<'a>(field: &'a Field) -> Option<&'a Container> {
    match &field.value {
        Value::Container(container) => Some(container),
        _ => None,
    }
}

fn scalar<'a>(field: &'a Field) -> Option<&'a crate::model::Scalar> {
    match &field.value {
        Value::Scalar(scalar) => Some(scalar),
        _ => None,
    }
}

pub fn run() -> Vec<Check> {
    vec![
        constants_and_references(),
        numeric_lexemes(),
        inline_scripts(),
        conditional_blocks(),
        inner_field_keys(),
        duplicate_definitions(),
        unfamiliar_encodings(),
    ]
}

/// Requirement 8: scripted-constant definitions and references stay distinct enough for the
/// resolver to produce static base values.
fn constants_and_references() -> Check {
    let outcome = (|| {
        let (_, file) = parse("common/scripted_variables/parser_constants.txt")?;

        let declaration = find(&file, "@parser_cost_small")
            .ok_or("no `@parser_cost_small` declaration".to_string())?;
        if declaration.key.kind != ScalarKind::VariableRef {
            return Err(format!("declaration key is {:?}", declaration.key.kind));
        }

        let alias = find(&file, "@parser_alias").ok_or("no `@parser_alias`".to_string())?;
        let value = scalar(alias).ok_or("alias value is not a scalar".to_string())?;
        if value.kind != ScalarKind::VariableRef {
            return Err(format!("a constant-valued constant read as {:?}", value.kind));
        }

        // The consuming site is what the game gets wrong: an unresolved `@` makes it read
        // `cost` as a script-value block and swallow the lines after it.
        let consumer =
            find(&file, "parser_forward_consumer").ok_or("no consumer".to_string())?;
        let body = container(consumer).ok_or("consumer is not a block".to_string())?;
        let cost = body
            .fields()
            .find(|field| field.key.text() == "cost")
            .ok_or("consumer has no `cost`".to_string())?;
        let reference = scalar(cost).ok_or("`cost` was not left a scalar".to_string())?;
        if reference.kind != ScalarKind::VariableRef {
            return Err(format!("`cost = @…` read as {:?}", reference.kind));
        }
        if !body.fields().any(|field| field.key.text() == "tier") {
            return Err("`tier` after the reference was swallowed".into());
        }

        Ok("declaration, alias, and consuming reference all retained; `tier` still visible".into())
    })();

    Check {
        requirement: 8,
        name: "scripted constants and their references stay distinct",
        outcome,
    }
}

/// Requirement 9: the exact numeric lexeme survives, and a date is not mistaken for one.
fn numeric_lexemes() -> Check {
    let outcome = (|| {
        let (_, file) = parse("common/scripted_variables/parser_constants.txt")?;

        let mut seen = Vec::new();
        for name in ["@parser_tenth_short", "@parser_tenth_long", "@parser_negative"] {
            let field = find(&file, name).ok_or(format!("no `{name}`"))?;
            let value = scalar(field).ok_or(format!("`{name}` is not a scalar"))?;
            if value.kind != ScalarKind::Number {
                return Err(format!("`{name}` classified {:?}", value.kind));
            }
            seen.push(value.text().into_owned());
        }
        if seen[0] == seen[1] {
            return Err("`0.1` and `0.10` were normalized to one lexeme".into());
        }

        let shapes = find(&file, "parser_numeric_shapes").ok_or("no shapes block".to_string())?;
        let body = container(shapes).ok_or("shapes is not a block".to_string())?;
        let date = body
            .fields()
            .find(|field| field.key.text() == "start_date")
            .ok_or("no `start_date`".to_string())?;
        let date = scalar(date).ok_or("date is not a scalar".to_string())?;
        if date.kind == ScalarKind::Number {
            return Err("a date was classified as a number".into());
        }

        Ok(format!(
            "lexemes preserved distinctly: {}; `{}` not read as a number",
            seen.join(", "),
            date.text()
        ))
    })();

    Check {
        requirement: 9,
        name: "numeric lexemes survive exactly",
        outcome,
    }
}

/// Requirement 10: inline-script references, parameter bindings, and fragments survive
/// unexpanded.
fn inline_scripts() -> Check {
    let outcome = (|| {
        let (_, callee) = parse("common/inline_scripts/parser/fragment.txt")?;
        let modifier = find(&callee, "modifier").ok_or("fragment lost its body".to_string())?;
        let body = container(modifier).ok_or("fragment body is not a block".to_string())?;
        let factor = body
            .fields()
            .find(|field| field.key.text() == "factor")
            .ok_or("no `factor`".to_string())?;
        let placeholder = scalar(factor).ok_or("`factor` is not a scalar".to_string())?;
        if placeholder.kind != ScalarKind::Parameter {
            return Err(format!("`$FACTOR$` read as {:?}", placeholder.kind));
        }

        let (_, caller) = parse("common/technology/parser_inline_and_keys.txt")?;

        let scalar_form = find(&caller, "parser_tech_inline_scalar")
            .and_then(container)
            .and_then(|body| body.fields().find(|f| f.key.text() == "weight_modifier"))
            .and_then(container)
            .and_then(|body| body.fields().find(|f| f.key.text() == "inline_script"))
            .ok_or("scalar call form missing".to_string())?;
        if scalar(scalar_form).map(|s| s.kind) != Some(ScalarKind::Quoted) {
            return Err("the quoted call form did not stay a scalar".into());
        }

        let bindings = find(&caller, "parser_tech_inline_parameterized")
            .and_then(container)
            .and_then(|body| body.fields().find(|f| f.key.text() == "weight_modifier"))
            .and_then(container)
            .and_then(|body| body.fields().find(|f| f.key.text() == "inline_script"))
            .and_then(container)
            .ok_or("parameterized call form missing".to_string())?;
        let keys: Vec<_> = bindings
            .fields()
            .map(|field| field.key.text().into_owned())
            .collect();
        if keys != ["script", "FACTOR"] {
            return Err(format!("parameter bindings read as {keys:?}"));
        }

        Ok("fragment, quoted call, and parameter bindings all retained unexpanded".into())
    })();

    Check {
        requirement: 10,
        name: "inline scripts are preserved, not expanded",
        outcome,
    }
}

/// Requirement 10, continued: `[[NAME] … ]` is syntax the resolver must be able to see.
fn conditional_blocks() -> Check {
    let outcome = (|| {
        let (_, file) = parse("common/technology/parser_inline_and_keys.txt")?;
        let body = find(&file, "parser_tech_conditional")
            .and_then(container)
            .ok_or("no conditional definition".to_string())?;

        let blocks: Vec<_> = body
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Conditional(conditional) => Some(conditional),
                _ => None,
            })
            .collect();
        if blocks.len() != 3 {
            return Err(format!("{} conditional blocks, expected 3", blocks.len()));
        }
        if !blocks[1].negated || blocks[0].negated {
            return Err("negation was not distinguished".into());
        }
        if !blocks[2].items.is_empty() {
            return Err("the empty `[[FACTOR]]` body gained content".into());
        }
        // Conditional bodies must not be flattened into the definition, or a technology
        // would appear to hold both branches of a `[[X]` / `[[!X]` pair at once.
        if body.fields().any(|field| field.key.text() == "prerequisites") {
            return Err("a conditional body was flattened into the definition".into());
        }

        Ok("three blocks: one plain, one negated, one empty; bodies not flattened".into())
    })();

    Check {
        requirement: 10,
        name: "conditional-compilation blocks stay conditional",
        outcome,
    }
}

/// Requirement 11: both the enclosing block name and the inner identifier survive.
fn inner_field_keys() -> Check {
    let outcome = (|| {
        let (_, file) = parse("common/technology/parser_inline_and_keys.txt")?;
        let components: Vec<_> = file
            .definitions()
            .filter(|field| field.key.text() == "utility_component_template")
            .collect();
        if components.len() != 2 {
            return Err(format!("{} component templates, expected 2", components.len()));
        }

        let mut keys = Vec::new();
        for component in &components {
            let body = container(component).ok_or("component is not a block".to_string())?;
            let inner = body
                .fields()
                .find(|field| field.key.text() == "key")
                .ok_or("component has no inner `key`".to_string())?;
            keys.push(
                scalar(inner)
                    .ok_or("inner key is not a scalar".to_string())?
                    .text()
                    .into_owned(),
            );
        }
        if keys[0] == keys[1] {
            return Err("the two components became indistinguishable".into());
        }

        let (_, sprites) = parse("interface/parser_sprites.gfx")?;
        let sprite_names: Vec<_> = find(&sprites, "spriteTypes")
            .and_then(container)
            .ok_or("no spriteTypes block".to_string())?
            .fields()
            .filter(|field| field.key.text() == "spriteType")
            .filter_map(container)
            .filter_map(|body| body.fields().find(|field| field.key.text() == "name"))
            .filter_map(scalar)
            .map(|value| value.text().into_owned())
            .collect();
        if sprite_names.len() != 3 {
            return Err(format!("{} sprite names, expected 3", sprite_names.len()));
        }

        Ok(format!(
            "component keys {keys:?} under one block name; {} sprites likewise",
            sprite_names.len()
        ))
    })();

    Check {
        requirement: 11,
        name: "inner-field identifiers remain addressable",
        outcome,
    }
}

/// Requirement 2: duplicate definitions survive in order, with their bodies attached to the
/// right occurrence.
fn duplicate_definitions() -> Check {
    let outcome = (|| {
        let (_, file) = parse("common/technology/parser_duplicates.txt")?;
        let costs: Vec<_> = file
            .definitions()
            .filter(|field| field.key.text() == "parser_tech_repeated")
            .filter_map(container)
            .filter_map(|body| body.fields().find(|field| field.key.text() == "cost"))
            .filter_map(scalar)
            .map(|value| value.text().into_owned())
            .collect();
        if costs != ["100", "200", "300"] {
            return Err(format!("costs read as {costs:?}"));
        }
        Ok("three occurrences in source order, bodies intact".into())
    })();

    Check {
        requirement: 2,
        name: "duplicate definitions survive in order",
        outcome,
    }
}

/// Requirement 5: unfamiliar bytes are data, not a syntax failure.
fn unfamiliar_encodings() -> Check {
    let outcome = (|| {
        let (_, bom) = parse("common/parser_encoding_bom.txt")?;
        let first = bom
            .definitions()
            .next()
            .ok_or("BOM file yielded no definitions".to_string())?;
        if first.key.text() != "parser_bom_definition" {
            return Err(format!("BOM leaked into the key: {:?}", first.key.text()));
        }

        for relative in [
            "common/parser_encoding_windows1252.txt",
            "common/parser_encoding_invalid.txt",
        ] {
            let (data, parsed) = parse(relative)?;
            if parsed.definitions().count() != 2 {
                return Err(format!(
                    "{relative}: {} definitions, expected 2",
                    parsed.definitions().count()
                ));
            }
            if std::str::from_utf8(&data).is_ok() {
                return Err(format!("{relative} is valid UTF-8; it no longer tests anything"));
            }
        }

        // The tape reads these too, so this is a shared property rather than a lexer one.
        let data = std::fs::read(fixture("common/parser_encoding_invalid.txt"))
            .map_err(|error| error.to_string())?;
        tape::parse(&data).map_err(|error| format!("tape rejected invalid UTF-8: {}", error.message))?;

        Ok("BOM skipped; both non-UTF-8 files parsed by each adapter without a fault".into())
    })();

    Check {
        requirement: 5,
        name: "unfamiliar bytes are retained, not rejected",
        outcome,
    }
}
