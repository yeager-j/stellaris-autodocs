# Localization lexical census

Status: Complete. Measured against Stellaris `Pegasus v4.4.6 (fdde)` (`modsCompatibilityVersion` `4.4`) and Ancient Cache of Technologies, Workshop `1419304439`. Settles the `.yml` lexical rules STE-37 had to choose without an oracle record.

## The question

No captured record measures Stellaris `.yml` lexis. `r13`, `r14` and `r15` establish which localization files load in which order and what an exact-path collision destroys; none of them says whether a value ends at the first closing quote or the last, whether the `:<version>` suffix is mandatory, whether a byte-order mark is required, or what the game does with a line that has no quoted value at all.

Phase 5A had to answer all four to read a single key. Answering them from the wiki would repeat the mistake `r13` was run to correct — a community wiki is a hypothesis — and answering them from taste would put a rule nothing can re-check between the mod's bytes and a player-visible name. So they were answered by measuring the corpus, and this is where that measurement stays checkable.

## Method

```
cargo test --features test-support localization_lexical_census -- --ignored --nocapture
```

The run lives at `src-tauri/src/analysis/localization_stage/census.rs`; its module comment is the contract. Two properties of the method matter more than the counts:

**Interpretation is `localization`'s decision, not the census's.** The corpus is read through the production path — `localization_stage::tables` over a real `Resolution` — and reported through `localization`'s public surface. A regex over the bytes would be a second authority on the grammar, which is the argument the inline-parameter census already makes about tokenization.

**The one independent counter is deliberately coarser than the grammar.** To claim that nothing was read and silently dropped, the census needs a floor for what interpretation *owes* an answer about, and it cannot ask the parser for that without asking the accused to testify. So it counts bytes instead: a line is key-shaped if it is not a comment and holds both a colon and a quote. A counter that agreed with the parser by construction could not catch the parser losing a line.

## What the corpus holds

2,364 surviving localization files — Vanilla's 2,318 and ACOT's 46 — and **zero removed**: ACOT ships its overrides under `localisation/english/replace/`, so nothing collides with a vanilla path and the corpus produces no raw-key casualties at all. The r14 casualty machinery is therefore exercised by fixtures rather than by this corpus, which is itself worth knowing: the hazard is real but the pinned Target Mod does not trigger it.

| | Vanilla | ACOT |
| --- | ---: | ---: |
| Files | 2,318 | 46 |
| With a byte-order mark | 2,318 | 46 |
| Using CRLF terminators | 3 | 41 |
| Key lines with a `:<version>` suffix | 109,018 | 5 |
| Key lines without one | 1,397,543 | 16,788 |
| Files holding no key-shaped line at all | 31 | 0 |
| Effective keys | 1,506,417 across 10 languages | 16,727 English |
| Values shadowed by a later statement | 144 | 64 |
| Values with an unescaped inner quote | 1,477 | 152 |
| Conditions | **0** | 3 |

Effective keys per language sit between 149,097 (English) and 160,741 (French); the other eight cluster within 1,800 of English.

### The version suffix is the exception, not the rule

**92.8% of vanilla key lines omit `:<version>` entirely**, and ACOT omits it on all but five of 16,793. A grammar that required it would have failed on 1.4 million lines; one that did not know it existed would have read `materials:0 "Materials"` as an unquoted value and dropped it. It is parsed and discarded: no consumer — fallback, reference resolution, the cited-key closure, the tokenizer, search — reads a version, and a retained field with no reader is what "model only what somebody reads" forbids.

### A value ends at the last quote, not the next one

**1,477 vanilla lines and 152 ACOT lines carry an unescaped inner double quote.** They are ordinary player-visible prose:

```
 action.76.desc:0 "Governor [malfunctioning_leader.GetName] has suffered a critical malfunction - "bricking" - following …"
 FEN_HABBANIS_DESC:0 "Fen Habbanis III, or simply "Fen Habbanis" as it was known to its ancient inhabitants, was the capital of the First League."
 anomaly_failure.25.desc:0 "The "city ruins" that were reported on the surface of [From.From.GetName] turned out to be …"
```

A rule ending the value at the *next* quote reads `Governor [malfunctioning_leader.GetName] has suffered a critical malfunction - ` and silently loses the rest of a sentence a player reads. Meanwhile 12,702 vanilla lines carry a `#` comment after the closing quote, so scanning to end of line swallows a comment into a name. First-quote-to-last-quote is the only rule that survives both, and its residual is bounded rather than argued away — see the third claim below.

### Malformed lines are a mod phenomenon

The base game produced **no condition of any kind** across 1.5 million key lines. ACOT produced exactly three, all in its `replace/` directory:

| Condition | Site | Line |
| --- | --- | --- |
| `UnquotedValue` | `acot_00_components_weapons_l_english.yml:1926` | `ACOT_SC_GUNSHIP_4_DESC: Gunship"` |
| `UnterminatedValue` | `acot_00_herculean_events_l_english.yml:219` | `acot_herculean_built_score: "§EHerculean Built§!` |
| `UnquotedValue` | `acot_05_the_shadow_events_l_english.yml:40` | `acot_omegan_blessed: Blessed By Light` |

Each is a typed condition carrying the file, the line, **and the key the line named** — `ACOT_SC_GUNSHIP_4_DESC`, `acot_herculean_built_score`, `acot_omegan_blessed` — and each costs exactly one key: the line after it parses normally. The key is retained because a consumer holding only a file and a line could not tell a malformed definition from a key nobody ever defined without reparsing the source, and "the definition of `x` is malformed at line n" is the whole content of the Analysis Issue this is for. That is the whole reason ingestion is total. Refusing a build over these three lines would refuse documentation for a 16,727-key mod, and dropping them silently would leave three names rendering as raw keys with nothing anywhere to say why.

### Two structural shapes that are not errors

**A file may hold more than one language.** `localisation/languages.yml` holds ten `l_<language>:` sections in one file — every other file in either corpus holds exactly one. A parser that took locale identity from the first header, or from the file name, would file nine languages' worth of language names under English. Sections switch the current language, with no special case for the file.

**A file may hold no keys.** 31 vanilla files are a language header and comments, or a header alone: `english/preftl_events_l_english.yml` is fourteen bytes. `braz_por/new_scripted_loc_POR_l_braz_por.yml` goes further and has no header either — every line, including its `l_braz_por:` header, is commented out by a localization editor that flagged the file redundant. Both are benign, real base-game content, and neither is a condition.

### A language is whatever a header spells, and the header decides it

The parse is syntactic: `LanguageTag` accepts any `l_<name>`, so a translation mod that adds a language gets a table rather than a pile of skipped keys, and an eleventh base-game language would simply appear (D-135's one-vocabulary rule; the type is STE-39's, and STE-37 parses `.yml` headers through it as the second of the two to land). What is left for the unreadable-header condition is a header the game itself could not read as one — `l_English:`, `l_日本語:`, `l_english-uk:`. **Zero of those occur in either corpus**, so the condition is currently vacuous and covered by unit tests alone.

The census still asserts the base game's language set, but as a *reading* rather than a constraint: exactly the ten `languages.yml` declares. It is the guard a header-parsing regression trips first — if headers stopped being read, that set would shrink long before any key count looked wrong — and a game update that adds a language fails it, which is the signal to re-read this note.

### The header, not the path, decides

Not from these two corpora — both are internally consistent — but the counterexample exists and is why the rule is stated as a rule. Workshop mod `3039370479` ships `localisation/braz_por/PreSelect_l_braz_por.yml` whose section header is `l_english:`. Attributing by directory would file its keys under a language the player would never see them in. The parser is given bytes and no path, so it structurally cannot consult one.

## Conclusion

**All four lexical rules fit the corpus exactly, and each of them has a shape in the corpus that would have caught a plausible alternative.** The suffix is optional (1.4M lines), the value runs to the last quote (1,477 lines), the mark is present but must not be required (nine Gigastructural Engineering files ship without one — see below), and malformed input is real but rare and always mod-side (3 lines).

The one rule not exercised by either pinned corpus is stripping an absent byte-order mark: every file in both carries one. The mark-less shape was measured elsewhere during STE-37 — nine files in Gigastructural Engineering (`1121692237`) ship without it — and asserting its presence in this census would be asserting something about a mod the pinned corpora do not contain. It is exercised instead by `fixtures/resolver/localization-vanilla/localisation/english/main_2_l_english.yml`, which is committed mark-less and CRLF and runs in ordinary CI. The census still reports the count, so a corpus that gained one becomes visible.

## Why the assertions, and why none of them passes vacuously

The counts above are printed, not pinned: they are a property of whichever build and mod version is installed, and this note is where a specific reading belongs. Four claims are asserted, because each would be a changed *conclusion*:

1. **No file that holds a key-shaped line produced neither a key nor a condition.** The anti-silent-drop invariant at corpus scale, where the property tests state it over generated input. Measured against the independent byte-level floor described above, which is why the 31 empty files satisfy it rather than failing it.
2. **The base game produces no condition at all.** A fault in Paradox's own files means the grammar is wrong, not that the file is.
3. **The value rule leaves no residual.** No `TrailingContentAfterValue` anywhere — nothing sits after a value's last quote but a comment — and no captured value carries the signature of having swallowed a comment that itself contained a quote (a `"` followed by `#`, the only way the last-quote rule can be wrong). Both are zero across 1.5 million lines. If either ever fails, the rule becomes "the last quote before an unquoted `#`" and `LOCALIZATION_INTERPRETATION_VERSION` bumps with it.
4. **Every shape the rules exist for still occurs, and the detectors are live.** A byte-order mark, CRLF terminators, versioned *and* unversioned key lines, unescaped inner quotes, and at least one malformed line in the mod corpus. Without this, claims 2 and 3 could be reporting a dead detector rather than a clean corpus.
5. **The base game ships exactly the ten languages its `languages.yml` declares.** A reading of the pinned build, not a constraint on what a language may be, and the guard a header-parsing regression trips first.

Claim 1 has been shown to go red. It was written first in the stronger form "every file produced something", which failed on the 31 header-only base-game files:

```
31 localization files produced neither a key nor a condition:
  ["localisation/braz_por/new_scripted_loc_POR_l_braz_por.yml",
   "localisation/braz_por/preftl_events_l_braz_por.yml", …]
```

That failure is what produced the byte-level floor and the sharper claim; the empty-file case is now a counted, reported fact rather than an unexplained pass.

## What this does not settle

Three questions the corpus cannot answer, each held as a typed condition rather than a guess:

- **What the game does with a malformed line.** ACOT's three lines are dropped here. Whether the game drops them, reads them literally, or repairs them is unmeasured; no record covers it, and inventing a value would fabricate a player-visible name. Settling it needs a run of `r13`'s shape with a deliberately malformed subject.
- **What the game does with a header it cannot read as a language.** Zero occurrences across both corpora and the twenty Workshop mods surveyed. The section is skipped and its key count recorded; whether the game skips it, reads it as some default, or refuses the file is unmeasured, and the same run that would settle a malformed line would settle this.
- **Whether `\"` is unescaped for display.** 12,062 vanilla lines use it. Ingestion retains it verbatim along with `§`, `£` and `$…$`, because the markup tokenizer (Phase 5 task 2) is the one authority on what any of them mean and unescaping here would hand it a second dialect to accept.
