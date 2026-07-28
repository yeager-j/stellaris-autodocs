//! The divergences between the two readings that are documented behaviour, not open defects.
//!
//! Some disagreements are the tape being wrong in ways the adapter deliberately does not
//! reproduce. Driving this table to zero would mean reproducing those misreadings in the
//! adapter, which would be worse work than carrying the list
//! (`docs/spikes/parser-evaluation.md`, "Driving the divergence count to zero"). **The count
//! is accounting, not a score.**
//!
//! Each entry names the file, what was observed, the class, and the trace that justified it.
//! An entry is added only after the disagreement has been traced to a specific tape
//! behaviour — never to make a run go green.
//!
//! Reconciliation is two-sided. A divergence that is not pinned fails the run, and a pinned
//! divergence that no longer occurs also fails it: a table that could only ever be too small
//! would rot silently as the corpora and the adapter change.

use super::tape::Divergence;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ExpectedDivergence {
    pub(super) corpus: &'static str,
    pub(super) logical: &'static str,
    /// The projection path, as [`Divergence::path`] renders it.
    pub(super) path: &'static str,
    /// What was observed there. Pinned as well as the path so the entry records the
    /// measurement rather than only its location: a file whose contents shift under a game
    /// update diverges by a different amount, and that is worth failing on.
    pub(super) detail: &'static str,
    pub(super) class: &'static str,
    /// Why this is the tape being wrong rather than the adapter.
    pub(super) trace: &'static str,
}

impl ExpectedDivergence {
    fn matches(&self, logical: &str, divergence: &Divergence) -> bool {
        self.logical == logical && self.path == divergence.path && self.detail == divergence.detail
    }
}

/// The tape never tokenizes `=`: `TextToken::Operator` is documented as carrying only a
/// *non-equal* operator. Inside an object that is harmless, because position implies the
/// pairing. Inside a container the tape does not treat as an object — one bare token
/// anywhere in it is enough — a scalar-to-scalar assignment is therefore **absent from the
/// tape**, and `key = value` cannot be told from two bare tokens.
///
/// So this class is not two plausible readings. The adapter is right and the tape cannot be,
/// because the evidence is not in it. It is also the sharpest available statement of why
/// `docs/adr/0007` reads Stellaris source through `TokenReader` instead.
const ELIDED_EQUALS_LOST: &str = "scalar assignment lost where the tape drops `=`";

/// The same root cause reaching a different construct: inside a non-object container the tape
/// stops emitting `Parameter` tokens for `[[NAME] … ]`, so a conditional-compilation block
/// arrives as bare tokens.
const CONDITIONAL_LOST: &str = "conditional block lost in a non-object container";

/// The tape splits `[Scope.GetName]` at its brackets. Rejoining the parts works until a
/// `MixedContainer` marker lands between them.
const SCOPE_TOKEN_SPLIT: &str = "scope token split by the tape";

/// Pinned against Stellaris `Pegasus v4.4.6 (fdde)` and ACOT `1419304439`, derived by tracing
/// the run rather than by transcribing the spike's list: the production adapter is not the
/// spike's lexer, so its disagreements with the tape had to be established again. Three of
/// the spike's seven classes survive here; the rest belonged to corpora this run does not
/// cover. See `docs/conformance/parser/` for the run that produced them.
pub(super) const EXPECTED: &[ExpectedDivergence] = &[
    ExpectedDivergence {
        corpus: "fixtures",
        logical: "interface/parser_sprites.gfx",
        path: "<root>[1](parser_scope_tokens).value",
        detail: "item count 4 vs 2",
        class: SCOPE_TOKEN_SPLIT,
        trace: "\
The tape splits `[Root.GetName]` into `[`, the body, and `]`, and puts a `MixedContainer` \
marker between the bracket and the body (tokens 39-42 of this file). The marker is what makes \
the split unrecoverable here: rejoining stops at the first token that is not a scalar, so the \
second reading ends up with `simple = \"[\"` followed by two bare elements where the adapter \
has one scope-token scalar. The adapter takes the token whole from the bytes. The fixture's \
own header records the same observation.",
    },
    ExpectedDivergence {
        corpus: "vanilla",
        logical: "common/scripted_effects/02_machine_age_effects.txt",
        path: "<root>[25](synth_queen_spawn_history_projects).value",
        detail: "item count 10 vs 8",
        class: CONDITIONAL_LOST,
        trace: "\
The definition opens with the bare token `optimize_memory`, so the tape does not treat its \
body as an object, and inside a non-object container it emits the conditional-compilation \
block `[[NUM_EXTRA_PROJECTS] while = { … } ]` as three bare tokens rather than as the \
`Parameter` token it uses elsewhere. The second reading therefore reports `[[NUM_EXTRA_\
PROJECTS]`, a `while` field, and `]` where the adapter reports one conditional. The adapter \
lexes the construct from the bytes, so the enclosing container's shape cannot affect it.",
    },
    ExpectedDivergence {
        corpus: "vanilla",
        logical: "interface/reference.txt",
        path: "<root>",
        detail: "item count 111 vs 93",
        class: ELIDED_EQUALS_LOST,
        trace: "\
A hand-maintained reference sheet rather than loaded script: line 6 is the bare token \
`GFX_default_fallback_texture`, which stops the tape treating the file's root as an object. \
Every later `key = value` whose value is a scalar then becomes two bare elements, for the \
reason the class note gives. Nine such pairs account for the eighteen-item gap.",
    },
    ExpectedDivergence {
        corpus: "acot",
        logical: "common/component_templates/acot_1_components_weapons_mutations_delta.txt",
        path: "<root>",
        detail: "item count 336 vs 326",
        class: ELIDED_EQUALS_LOST,
        trace: "\
The file begins with the literal bytes `3407` before its first comment — garbage in shipped \
source that leaves the file syntactically valid, so neither reading reports a fault. The \
stray token stops the tape treating the root as an object, and the ten scalar-to-scalar \
scripted-constant assignments that follow (`@attack_range = 8` and its neighbours) each \
become two bare elements, which is the whole of the 336-versus-326 gap. **This is the finding \
the cross-check exists for**: no single reading can see it, and it is why \
`fixtures/parser/malformed/stray_token_at_head.txt` exists.",
    },
];

/// What the observed divergences say against the pinned table, for the corpora that ran.
#[derive(Debug, Default)]
pub(super) struct Reconciliation {
    pub(super) unexpected: Vec<String>,
    pub(super) absent: Vec<String>,
}

impl Reconciliation {
    pub(super) fn is_clean(&self) -> bool {
        self.unexpected.is_empty() && self.absent.is_empty()
    }
}

/// Reconciles one corpus's observed divergences against the entries pinned for it.
///
/// Scoped per corpus so a run over a subset does not report every other corpus's pinned
/// entries as absent.
pub(super) fn reconcile(corpus: &str, observed: &[(String, Divergence)]) -> Reconciliation {
    let pinned: Vec<&ExpectedDivergence> = EXPECTED
        .iter()
        .filter(|expected| expected.corpus == corpus)
        .collect();

    let unexpected = observed
        .iter()
        .filter(|(logical, divergence)| {
            !pinned
                .iter()
                .any(|expected| expected.matches(logical, divergence))
        })
        .map(|(logical, divergence)| {
            format!(
                "{corpus}\t{logical}\t{}: {}",
                divergence.path, divergence.detail
            )
        })
        .collect();

    let absent = pinned
        .iter()
        .filter(|expected| {
            !observed
                .iter()
                .any(|(logical, divergence)| expected.matches(logical, divergence))
        })
        .map(|expected| {
            format!(
                "{corpus}\t{}\t{} ({}) no longer diverges as `{}`",
                expected.logical, expected.path, expected.class, expected.detail
            )
        })
        .collect();

    Reconciliation { unexpected, absent }
}

#[test]
fn every_pinned_divergence_names_a_known_class_and_carries_its_trace() {
    // The table's discipline is that an entry is added only after the disagreement has been
    // traced. A test cannot check that the prose is true, but it can stop an entry being
    // added with the justification left blank, and it can stop a one-off class name being
    // invented to make an unexplained divergence look accounted for.
    for expected in EXPECTED {
        assert!(
            [ELIDED_EQUALS_LOST, CONDITIONAL_LOST, SCOPE_TOKEN_SPLIT].contains(&expected.class),
            "{} pins an unrecognized class {:?}",
            expected.logical,
            expected.class
        );
        assert!(
            expected.trace.len() > 80,
            "{} pins a divergence without tracing it",
            expected.logical
        );
    }
}
