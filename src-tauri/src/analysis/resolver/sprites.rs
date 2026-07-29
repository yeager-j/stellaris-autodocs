//! Sprite-specific resolution after the generic registry has selected every winning body.
//!
//! Registration and reference resolution are deliberately separate passes. r17 established
//! that `sprite_sheet_sprite_type` reads the winning named sprite: a Target Mod override of
//! one sheet changed the effective texture of 54 Vanilla dependents. Following references
//! during the stream walk would make an early dependent see whichever definition happened
//! to be registered at that moment rather than the final winner.

use std::collections::{BTreeMap, BTreeSet};

use super::super::parser::Value;
use super::resolved::{
    DefinitionKey, FactSite, ResolvedDefinition, ResolvedSpriteTexture, SpriteReferenceEdge,
    SpriteResolution, SpriteTextureOutcome,
};

pub(super) const SPRITE_SHEET: &str = "sprite_sheet_sprite_type";
const PRIMARY_TEXTURE: &str = "texturefile";

pub(super) fn attach(definitions: &mut BTreeMap<DefinitionKey, ResolvedDefinition>) {
    let resolutions = definitions
        .keys()
        .cloned()
        .map(|key| {
            let resolution = resolve_one(definitions, &key);
            (key, resolution)
        })
        .collect::<BTreeMap<_, _>>();

    for (key, resolution) in resolutions {
        definitions
            .get_mut(&key)
            .expect("a key collected from the registry still exists")
            .sprite = Some(resolution);
    }
}

fn resolve_one(
    definitions: &BTreeMap<DefinitionKey, ResolvedDefinition>,
    start: &DefinitionKey,
) -> SpriteResolution {
    let mut visited = BTreeSet::from([start.clone()]);
    let mut current = start;
    let mut pending_edges = Vec::new();

    let outcome = loop {
        let definition = definitions
            .get(current)
            .expect("sprite resolution starts from and follows registry keys");

        if let Some(reference) = definition
            .fields
            .iter()
            .find(|field| field.field == SPRITE_SHEET)
        {
            let referenced = scalar_text(&reference.value);
            let target = referenced
                .as_deref()
                .and_then(|key| definitions.get(&DefinitionKey::new(key)));
            let target_site =
                target.map(|definition| FactSite::Stream(definition.position.clone()));

            let Some(target_definition) = target else {
                let outcome = SpriteTextureOutcome::MissingTarget {
                    sprite: referenced.clone(),
                };
                pending_edges.push(PendingEdge {
                    sprite: referenced,
                    site: reference.site.clone(),
                    target: None,
                });
                break outcome;
            };
            let target_key = target_definition.key.clone();
            pending_edges.push(PendingEdge {
                sprite: referenced,
                site: reference.site.clone(),
                target: target_site,
            });

            if !visited.insert(target_key.clone()) {
                break SpriteTextureOutcome::CyclicReference {
                    sprite: target_key.as_str().to_owned(),
                };
            }
            current = definitions
                .get_key_value(&target_key)
                .map(|(key, _)| key)
                .expect("the referenced target was read from this map");
            continue;
        }

        break definition
            .fields
            .iter()
            .find(|field| field.field == PRIMARY_TEXTURE)
            .and_then(|field| {
                scalar_text(&field.value).map(|path| {
                    SpriteTextureOutcome::Resolved(ResolvedSpriteTexture {
                        path,
                        site: field.site.clone(),
                    })
                })
            })
            .unwrap_or(SpriteTextureOutcome::MissingTexture);
    };

    let references = pending_edges
        .into_iter()
        .map(|edge| SpriteReferenceEdge {
            sprite: edge.sprite,
            site: edge.site,
            target: edge.target,
            outcome: outcome.clone(),
        })
        .collect();

    SpriteResolution {
        texture: outcome,
        references,
    }
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Scalar(value) => Some(value.text().into_owned()),
        Value::Container(_) | Value::Tagged { .. } => None,
    }
}

struct PendingEdge {
    sprite: Option<String>,
    site: FactSite,
    target: Option<FactSite>,
}
