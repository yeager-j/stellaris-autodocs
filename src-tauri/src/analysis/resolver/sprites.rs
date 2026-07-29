//! Sprite-specific resolution after the generic registry has selected every winning body.
//!
//! Registration and reference resolution are deliberately separate passes. r17 established
//! that `sprite_sheet_sprite_type` reads the winning named sprite: a Target Mod override of
//! one sheet changed the effective texture of 54 Vanilla dependents. Following references
//! during the stream walk would make an early dependent see whichever definition happened
//! to be registered at that moment rather than the final winner.

use std::collections::BTreeMap;

use super::super::parser::{ScalarKind, Value};
use super::resolved::{
    DefinitionKey, FactSite, ResolvedDefinition, ResolvedSpriteTexture, SpriteReferenceEdge,
    SpriteResolution, SpriteTextureOutcome,
};

pub(super) const SPRITE_SHEET: &str = "sprite_sheet_sprite_type";
const PRIMARY_TEXTURE: &str = "texturefile";

pub(super) fn attach(definitions: &mut BTreeMap<DefinitionKey, ResolvedDefinition>) {
    let outcomes = resolve_outcomes(definitions);
    let resolutions = definitions
        .iter()
        .map(|(key, definition)| {
            let texture = outcomes
                .get(key)
                .expect("every registry key receives a sprite texture outcome")
                .clone();
            let references = direct_reference(definitions, definition, &texture)
                .into_iter()
                .collect();
            (
                key.clone(),
                SpriteResolution {
                    texture,
                    references,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    for (key, resolution) in resolutions {
        definitions
            .get_mut(&key)
            .expect("a key collected from the registry still exists")
            .sprite = Some(resolution);
    }
}

fn resolve_outcomes(
    definitions: &BTreeMap<DefinitionKey, ResolvedDefinition>,
) -> BTreeMap<DefinitionKey, SpriteTextureOutcome> {
    let mut outcomes: BTreeMap<DefinitionKey, SpriteTextureOutcome> = BTreeMap::new();

    for start in definitions.keys() {
        if outcomes.contains_key(start) {
            continue;
        }

        let mut path: Vec<DefinitionKey> = Vec::new();
        let mut positions: BTreeMap<DefinitionKey, usize> = BTreeMap::new();
        let mut current = start.clone();

        let outcome = loop {
            if let Some(outcome) = outcomes.get(&current) {
                break outcome.clone();
            }

            if let Some(cycle_start) = positions.insert(current.clone(), path.len()) {
                let sprite = path[cycle_start..]
                    .iter()
                    .min()
                    .expect("a repeated key leaves a non-empty cycle")
                    .as_str()
                    .to_owned();
                break SpriteTextureOutcome::CyclicReference { sprite };
            }

            path.push(current.clone());
            let definition = definitions
                .get(&current)
                .expect("sprite resolution starts from and follows registry keys");
            match next_step(definitions, definition) {
                ResolutionStep::Follow(target) => current = target,
                ResolutionStep::Finished(outcome) => break outcome,
            }
        };

        for key in path {
            outcomes.insert(key, outcome.clone());
        }
    }

    outcomes
}

fn next_step(
    definitions: &BTreeMap<DefinitionKey, ResolvedDefinition>,
    definition: &ResolvedDefinition,
) -> ResolutionStep {
    if let Some(reference) = definition
        .fields
        .iter()
        .find(|field| field.field == SPRITE_SHEET)
    {
        return match scalar_text(&reference.value) {
            ScalarText::Literal(sprite) => {
                let target = DefinitionKey::new(&sprite);
                if definitions.contains_key(&target) {
                    ResolutionStep::Follow(target)
                } else {
                    ResolutionStep::Finished(SpriteTextureOutcome::MissingTarget {
                        sprite: Some(sprite),
                    })
                }
            }
            ScalarText::Unresolved(kind) => {
                ResolutionStep::Finished(SpriteTextureOutcome::UnresolvedScalar { kind })
            }
            ScalarText::NotScalar => {
                ResolutionStep::Finished(SpriteTextureOutcome::MissingTarget { sprite: None })
            }
        };
    }

    let outcome = definition
        .fields
        .iter()
        .find(|field| field.field == PRIMARY_TEXTURE)
        .map_or(
            SpriteTextureOutcome::MissingTexture,
            |field| match scalar_text(&field.value) {
                ScalarText::Literal(path) => {
                    SpriteTextureOutcome::Resolved(ResolvedSpriteTexture {
                        path,
                        site: field.site.clone(),
                    })
                }
                ScalarText::Unresolved(kind) => SpriteTextureOutcome::UnresolvedScalar { kind },
                ScalarText::NotScalar => SpriteTextureOutcome::MissingTexture,
            },
        );
    ResolutionStep::Finished(outcome)
}

fn direct_reference(
    definitions: &BTreeMap<DefinitionKey, ResolvedDefinition>,
    definition: &ResolvedDefinition,
    outcome: &SpriteTextureOutcome,
) -> Option<SpriteReferenceEdge> {
    let reference = definition
        .fields
        .iter()
        .find(|field| field.field == SPRITE_SHEET)?;
    let sprite = match scalar_text(&reference.value) {
        ScalarText::Literal(sprite) => Some(sprite),
        ScalarText::Unresolved(_) | ScalarText::NotScalar => None,
    };
    let target = sprite
        .as_deref()
        .and_then(|key| definitions.get(&DefinitionKey::new(key)))
        .map(|definition| FactSite::Stream(definition.position.clone()));

    Some(SpriteReferenceEdge {
        sprite,
        site: reference.site.clone(),
        target,
        outcome: outcome.clone(),
    })
}

fn scalar_text(value: &Value) -> ScalarText {
    match value {
        Value::Scalar(value) if matches!(value.kind, ScalarKind::Unquoted | ScalarKind::Quoted) => {
            ScalarText::Literal(value.text().into_owned())
        }
        Value::Scalar(value) => ScalarText::Unresolved(value.kind),
        Value::Container(_) | Value::Tagged { .. } => ScalarText::NotScalar,
    }
}

enum ResolutionStep {
    Follow(DefinitionKey),
    Finished(SpriteTextureOutcome),
}

enum ScalarText {
    Literal(String),
    Unresolved(ScalarKind),
    NotScalar,
}
