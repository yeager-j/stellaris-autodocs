//! The behavioural case table.
//!
//! Every case states the whole token sequence, so a case that changes shape fails rather than
//! drifting, and every case additionally runs the `spans` gate — that pairing is what makes
//! "nothing is silently discarded" structural instead of eyeballed.
//!
//! Cases live here as literals rather than as fixture files for two reasons. The invisible
//! characters that carry meaning — the no-break space gluing an icon to its label, a line
//! break inside a value — are written as `<nbsp>` and `<lf>` in the expectation, where a
//! reviewer can see them; in a file they would be indistinguishable from a space. And a `.yml`
//! fixture would pull ingestion into a suite that must not depend on it.
//!
//! Shapes are drawn from a census of the installed `Pegasus v4.4.6 (fdde)` corpus and the
//! workshop mods beside it, including every malformed family that build actually ships. The
//! identifiers are vanilla-shaped and the prose is written for this repository; no Stellaris
//! content is reproduced.

use super::scan::tokenize;
use super::spans::assert_tiles;
use super::token::{DisplayToken, TokenKind, VerbatimKind};

/// One line per token: kind, then payload. Spans are absent on purpose — `spans` owns them,
/// and repeating offsets here would make every case brittle against an unrelated edit.
fn describe(tokens: &[DisplayToken]) -> String {
    tokens
        .iter()
        .map(|token| match &token.kind {
            TokenKind::Text { text } => format!("text({})", quoted(text)),
            TokenKind::StyleOn { code } => format!("style({})", code.as_char()),
            TokenKind::StyleReset => "reset".to_owned(),
            TokenKind::Icon { name, variant } => match variant {
                Some(variant) => format!("icon({name}|{variant})"),
                None => format!("icon({name})"),
            },
            TokenKind::Reference { key, format } => match format {
                Some(format) => format!("ref({key}|{format})"),
                None => format!("ref({key})"),
            },
            TokenKind::Verbatim { kind, text } => {
                let name = match kind {
                    VerbatimKind::RuntimeToken => "runtime",
                    VerbatimKind::ConceptLink => "concept",
                    VerbatimKind::ScriptVariable => "variable",
                    VerbatimKind::DynamicIcon => "dynamic-icon",
                    VerbatimKind::UnpairedMarker => "unpaired",
                };
                format!("{name}({})", quoted(text))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders the characters that would otherwise be invisible or ambiguous in a test
/// expectation. Anything else is shown as itself, because the point of the notation is that a
/// reviewer can read the markup.
fn quoted(text: &str) -> String {
    let mut rendered = String::from("\"");
    for character in text.chars() {
        match character {
            '\u{a0}' => rendered.push_str("<nbsp>"),
            '\n' => rendered.push_str("<lf>"),
            '\t' => rendered.push_str("<tab>"),
            '"' => rendered.push_str("<quote>"),
            _ => rendered.push(character),
        }
    }
    rendered.push('"');
    rendered
}

#[track_caller]
fn tokenizes(value: &str, expected: &str) {
    let tokens = tokenize(value);
    assert_eq!(describe(&tokens), expected, "tokenizing {value:?}");
    assert_tiles(value, &tokens);
}

#[test]
fn text_alone_is_one_maximal_run() {
    tokenizes("", "");
    tokenizes(
        "Grants a research bonus.",
        r#"text("Grants a research bonus.")"#,
    );
    // The no-break space is the icon-to-label glue idiom and must not split a run: a scanner
    // that walked bytes without care would break this one at the 0xC2 lead byte.
    tokenizes("2\u{a0}Energy", r#"text("2<nbsp>Energy")"#);
    tokenizes("first\nsecond", r#"text("first<lf>second")"#);
    // A value the ingestion layer handed over without decoding escapes would look like this.
    // Nothing here interprets a backslash, which is the point.
    tokenizes(r"first\nsecond", r#"text("first\nsecond")"#);
    tokenizes(
        r#"a "quoted" aside"#,
        r#"text("a <quote>quoted<quote> aside")"#,
    );
    // `]` and `|` begin nothing, so they are ordinary text.
    tokenizes("a] b|c", r#"text("a] b|c")"#);
}

#[test]
fn style_markers_are_a_flat_stream() {
    tokenizes("§YHighlighted§!", r#"style(Y) text("Highlighted") reset"#);
    // Reset with nothing open, and a doubled reset. Both ship in vanilla; both are a no-op
    // against a current-style register and would need a repair rule against a stack.
    tokenizes("§!", "reset");
    tokenizes("§!§!", "reset reset");
    // An unterminated run. The game leaves it open to the end of the value.
    tokenizes("§RUnfinished", r#"style(R) text("Unfinished")"#);
    // Re-opening the outer colour by hand after an inner reset — the shape that makes a
    // nested-span model wrong. Nothing here pairs the markers with each other.
    tokenizes(
        "§ROuter §Yinner§!§R still outer§!",
        r#"style(R) text("Outer ") style(Y) text("inner") reset style(R) text(" still outer") reset"#,
    );
    // Lowercase codes are their own colours rather than aliases of the uppercase ones, and
    // digits and `_` are codes too. `interface/fonts.gfx` declares all of them.
    tokenizes("§cRare§!", r#"style(c) text("Rare") reset"#);
    tokenizes("§CCyan§!", r#"style(C) text("Cyan") reset"#);
    tokenizes("§0Tier§!", r#"style(0) text("Tier") reset"#);
    tokenizes("§_TODO§!", r#"style(_) text("TODO") reset"#);
}

#[test]
fn a_style_marker_with_no_code_is_one_unpaired_character() {
    // The three anomalous followers the installed build ships, all mistranslations. The `§`
    // survives visibly and the text after it tokenizes normally.
    tokenizes("§こ", r#"unpaired("§") text("こ")"#);
    tokenizes("§$KEY$§!", r#"unpaired("§") ref(KEY) reset"#);
    tokenizes("cost§ and", r#"text("cost") unpaired("§") text(" and")"#);
    // A `§` as the final character. Absent from the installed corpus, so the case exists to
    // pin the behaviour rather than to reproduce one.
    tokenizes("cost§", r#"text("cost") unpaired("§")"#);
}

#[test]
fn inline_icons_carry_a_name_and_an_opaque_variant() {
    tokenizes("£energy£", "icon(energy)");
    tokenizes("£MOD_SHIP_SPEED_MULT£", "icon(MOD_SHIP_SPEED_MULT)");
    tokenizes("£fleet_status|2£", "icon(fleet_status|2)");
    // The variant may itself be a reference. It stays opaque: the sprite table that gives
    // either half meaning arrives in Phase 8.
    tokenizes("£leader_skill|$LEVEL$£", "icon(leader_skill|$LEVEL$)");
    // The canonical idiom, and the reason the no-break space must survive tokenization.
    tokenizes(
        "£energy£\u{a0}Energy Credits",
        r#"icon(energy) text("<nbsp>Energy Credits")"#,
    );
    // Icons are colour-transparent; nothing nests them inside the style run.
    tokenizes(
        "§Y£time£3 Months§!",
        r#"style(Y) icon(time) text("3 Months") reset"#,
    );
}

#[test]
fn an_icon_selected_by_a_reference_is_raw_rather_than_a_name() {
    tokenizes("£$SHIP_ICON$£", r#"dynamic-icon("£$SHIP_ICON$£")"#);
    tokenizes(
        "£$RESOURCE_KEY$£ $VALUE$",
        r#"dynamic-icon("£$RESOURCE_KEY$£") text(" ") ref(VALUE)"#,
    );
}

#[test]
fn a_mispaired_icon_marker_costs_one_character_and_no_more() {
    // Shipped in vanilla: a no-break space slipped between the name and its closing `£`, so
    // the next `£` in the line pairs with the wrong opener. Recovery keeps the sentence.
    tokenizes(
        "paid in £energy\u{a0}£Energy Credits",
        r#"text("paid in ") unpaired("£") text("energy<nbsp>") unpaired("£") text("Energy Credits")"#,
    );
    // A doubled name, also shipped. The middle `£` opens a run that never closes.
    tokenizes(
        "£unity£unity£",
        r#"icon(unity) text("unity") unpaired("£")"#,
    );
    tokenizes("£", r#"unpaired("£")"#);
    // A run whose body is prose rather than a name. Rejecting the pairing lets the markup
    // inside it be recognized instead of swallowed.
    tokenizes(
        "£jobs: §G+2§!£",
        r#"unpaired("£") text("jobs: ") style(G) text("+2") reset unpaired("£")"#,
    );
}

#[test]
fn references_split_a_key_from_an_opaque_format() {
    tokenizes("$TECH_NAME$", "ref(TECH_NAME)");
    // Dotted keys are 132,296 occurrences across 2,314 distinct keys, and hyphens occur in
    // real keys too. Neither may be mistaken for a malformed body.
    tokenizes("$anomaly.6660.desc.start$", "ref(anomaly.6660.desc.start)");
    tokenizes(
        "$astral_rift.3135-3140.desc.common$",
        "ref(astral_rift.3135-3140.desc.common)",
    );
    tokenizes("$COST|0$", "ref(COST|0)");
    tokenizes("$DISCOUNT|%0$", "ref(DISCOUNT|%0)");
    tokenizes("$UPKEEP|0=+%$", "ref(UPKEEP|0=+%)");
    // Adjacent references share no separator: the `$$` is two delimiters, not an escape.
    tokenizes("$DIFFICULTY$$EXTRA|R$", "ref(DIFFICULTY) ref(EXTRA|R)");
    // A name-system slot. Its key is well-formed, so it is a reference whose lookup simply
    // will not find anything — this module promises shape, never existence.
    tokenizes("$1$ of $2$", r#"ref(1) text(" of ") ref(2)"#);
    // A Russian grammatical context tag. Same reasoning.
    tokenizes("$2&gen$", "ref(2&gen)");
}

#[test]
fn a_scripted_variable_is_withheld_from_reference_resolution() {
    // `@` names the scripted-variable namespace, so the replacement is not determinable from
    // localization files and this is definitionally not a Static Localization Reference.
    tokenizes(
        "$@default_monthly_progress$",
        r#"variable("$@default_monthly_progress$")"#,
    );
    tokenizes(
        "§R+$@living_standard_energy|*0$§!",
        r#"style(R) text("+") variable("$@living_standard_energy|*0$") reset"#,
    );
}

#[test]
fn a_mispaired_reference_marker_costs_one_character_and_no_more() {
    // An unterminated reference, shipped in a mistranslated file.
    tokenizes(
        "requires $CLASS|Y",
        r#"text("requires ") unpaired("$") text("CLASS|Y")"#,
    );
    // A body with no key. Both `$` end up unpaired and the suffix stays visible.
    tokenizes("$|Y$", r#"unpaired("$") text("|Y") unpaired("$")"#);
    // A body that swallowed a space is a mispairing, not a key.
    tokenizes("$5 or $6$", r#"unpaired("$") text("5 or ") ref(6)"#);
    // Shipped: a reference wrapped around a runtime token. Recovering one character at a time
    // recognizes the runtime token instead of hiding it.
    tokenizes(
        "$[Actor.GetName]$ has made Claims",
        r#"unpaired("$") runtime("[Actor.GetName]") unpaired("$") text(" has made Claims")"#,
    );
    // Shipped: an icon accidentally wrapped in reference delimiters.
    tokenizes("$£unity£$unity$", r#"unpaired("$") icon(unity) ref(unity)"#);
}

#[test]
fn runtime_tokens_are_preserved_whole_and_uninterpreted() {
    tokenizes("[Root.GetName]", r#"runtime("[Root.GetName]")"#);
    tokenizes(
        "[From.solar_system.GetName]",
        r#"runtime("[From.solar_system.GetName]")"#,
    );
    tokenizes(
        "[event_target:federation_winner.GetName]",
        r#"runtime("[event_target:federation_winner.GetName]")"#,
    );
    // A scripted-loc token has no scope chain at all.
    tokenizes(
        "[GetGalCommunityName]",
        r#"runtime("[GetGalCommunityName]")"#,
    );
    // A grammatical context tag, and the tag-sensitive variant form. Both keep their `|||`
    // separators and angle placeholders inside one opaque token, because nothing renders them.
    tokenizes(
        "[Owner.GetPreFTLLower&x]",
        r#"runtime("[Owner.GetPreFTLLower&x]")"#,
    );
    tokenizes(
        "[scientist.GetAXX::found|||fem:founded]",
        r#"runtime("[scientist.GetAXX::found|||fem:founded]")"#,
    );
    // Colouring a runtime token is done by wrapping it, never by a pipe suffix inside it.
    tokenizes(
        "§H[From.GetName]§! was lost",
        r#"style(H) runtime("[From.GetName]") reset text(" was lost")"#,
    );
}

#[test]
fn concept_links_are_classified_at_the_boundary_and_not_parsed_inside() {
    tokenizes("['concept_gateways']", r#"concept("['concept_gateways']")"#);
    tokenizes(
        "['building:building_organic_sanctuary']",
        r#"concept("['building:building_organic_sanctuary']")"#,
    );
    // The reference inside a concept link is deliberately unreachable: one opaque token makes
    // it structurally impossible for resolution to expand it, rather than a rule to remember.
    tokenizes(
        "['concept_astral_rift', $ASTRAL_RIFTS$]",
        r#"concept("['concept_astral_rift', $ASTRAL_RIFTS$]")"#,
    );
    tokenizes(
        "['technology:tech_habitat_1', £engineering_research£\u{a0}$tech_habitat_1$]",
        r#"concept("['technology:tech_habitat_1', £engineering_research£<nbsp>$tech_habitat_1$]")"#,
    );
    // Unquoted display prose, spaces and all.
    tokenizes(
        "['concept_pc_frozen', Frozen Worlds]",
        r#"concept("['concept_pc_frozen', Frozen Worlds]")"#,
    );
    // A nested bracket run inside the link. Depth counting is what closes the outer one.
    tokenizes(
        "['concept_roboticist', [roboticist.GetName]]",
        r#"concept("['concept_roboticist', [roboticist.GetName]]")"#,
    );
    // Padding between the bracket and the quote occurs in shipped files.
    tokenizes(
        "[ 'concept_fallen_empire', §CFallen Empire§! ]",
        r#"concept("[ 'concept_fallen_empire', §CFallen Empire§! ]")"#,
    );
    // Curly quotes are a mistranslation, not a form. Deliberately not special-cased: the run
    // is raw either way, and inventing a repair would be interpretation without evidence.
    tokenizes("[‘concept_ships’]", r#"runtime("[‘concept_ships’]")"#);
}

#[test]
fn an_unterminated_bracket_run_does_not_swallow_the_rest_of_the_value() {
    tokenizes("[Root.GetName", r#"unpaired("[") text("Root.GetName")"#);
    // The doubled-bracket shape, ~60 values in the installed build. `[[` is not modelled as an
    // escape for a literal `[` — see `markup`'s header. What matters here is that the
    // reference after it survives instead of being absorbed into a raw tail.
    tokenizes(
        "[[$INDEX$] $FLEET_NAME$ returns",
        r#"unpaired("[") runtime("[$INDEX$]") text(" ") ref(FLEET_NAME) text(" returns")"#,
    );
    tokenizes(
        "$TRAIT$ [[$DATE$]",
        r#"ref(TRAIT) text(" ") unpaired("[") runtime("[$DATE$]")"#,
    );
}

#[test]
fn the_families_nest_and_interleave_without_mis_slicing() {
    // The origin-tooltip archetype: a reference, an icon glued to a concept link, styled runs
    // and a line break in one value.
    tokenizes(
        "$t$- $AVAILABLE_BUILDINGS$ £building£\u{a0}['building:holding_organic_sanctuary']\n\
         - Start with §Y2§! £pop£\u{a0}§HOrganic Pops§!",
        concat!(
            r#"ref(t) text("- ") ref(AVAILABLE_BUILDINGS) text(" ") icon(building) "#,
            r#"text("<nbsp>") concept("['building:holding_organic_sanctuary']") "#,
            r#"text("<lf>- Start with ") style(Y) text("2") reset text(" ") icon(pop) "#,
            r#"text("<nbsp>") style(H) text("Organic Pops") reset"#,
        ),
    );
    // A style run reset and re-opened around an icon and a reference mid-sentence.
    tokenizes(
        "$TRIGGER_FAIL$§RRequires the £society£ §!§Y$tech_deciphering$§!§R technology.§!",
        concat!(
            r#"ref(TRIGGER_FAIL) style(R) text("Requires the ") icon(society) text(" ") reset "#,
            r#"style(Y) ref(tech_deciphering) reset style(R) text(" technology.") reset"#,
        ),
    );
    // Raw quotes inside a style run, which the ingestion layer hands over as-is.
    tokenizes(
        "§L\"We are not impressed.\"§!",
        r#"style(L) text("<quote>We are not impressed.<quote>") reset"#,
    );
}

#[test]
fn tokenization_is_deterministic_across_calls() {
    // The behavioural half of the property test: the same value tokenizes identically, so a
    // build cannot depend on iteration order or ambient state.
    for value in [
        "§Y£energy£\u{a0}$COST|0$§!",
        "['concept_gateways'] and [Root.GetName]",
        "£$ICON$£ $@variable$ [[$INDEX$]",
    ] {
        assert_eq!(tokenize(value), tokenize(value));
    }
}
