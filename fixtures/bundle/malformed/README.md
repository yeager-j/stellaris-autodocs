# Bundle fixtures

The malformed-source golden case for
[the revision bundle evaluation](../../docs/spikes/revision-bundle-evaluation.md).

**The harness that consumed these files is gone.** `tools/bundle-spike/` was deleted in Phase
4C ([STE-25](https://linear.app/unnamed-system/issue/STE-25)) under the implementation plan's
rule that a spike is deleted once the work it de-risked lands; the evaluation's provenance
note records why. The fixtures are retained rather than deleted with it: they encode a
correctness expectation about publication that Phase 6 onward still has to meet, stated below,
and re-deriving that shape later would be work for nothing. Nothing currently reads them.

`fixtures/parser/malformed/` already holds malformed source, and this fixture does not
replace it. Those files are flat `.txt` at the fixture root, which is the right shape for
asking what a parser does with a broken file and the wrong shape for asking what a *bundle*
does: nothing in them sits on a registry path, so nothing in them can make a registry's
entry set incomplete. The correctness check this fixture serves —

> The malformed-source case publishes Incomplete Documentation rather than a partially
> written bundle.

— is about registry completeness propagating into a published artifact, so the malformed
content has to live at `common/technology/` and be surrounded by content that survives.

The fault shapes are restated from `fixtures/parser/malformed/` rather than referenced, for
the reason `fixtures/parser/` states about the oracle fixtures: a fixture frozen against one
spike's evidence should not become a dependency of another spike's, or a change that is
correct for one silently invalidates the other.

## What each file is for

| File | Purpose |
| --- | --- |
| `common/technology/00_bundle_clean_tech.txt` | Four technologies with no fault anywhere. The negative control: incompleteness must not reach these. |
| `common/technology/01_bundle_broken_tech.txt` | An unclosed brace mid-file. Definitions before it are Clean, definitions after heuristic resynchronization are Recovered, and one is lost entirely. |
| `common/technology/02_bundle_truncated_tech.txt` | Ends mid-definition. The fault is last, so recovery can reach nothing and exactly one definition is lost. |
| `localisation/english/bundle_l_english.yml` | Names and descriptions. Deliberately omits one key so raw-key fallback is exercised. |
| `localisation/french/bundle_l_french.yml` | A strict subset of the English keys, so selected-language → English fallback is exercised without a rebuild. |

Every localization file carries a UTF-8 byte order mark, because every shipped Stellaris
localization file does and a fixture that omits it would exercise a reader the game never
requires.

## Expectations

- The revision publishes. A malformed registry file is a completeness fact, not a build
  failure.
- The technology registry's entry set is marked **incomplete**: a file that could not be
  fully read means the complete set for that registry cannot be established.
- `tech_bundle_clean_*` entries carry no entry-scoped issue. Registry-wide incompleteness
  does not propagate down into facts whose evidence was never affected.
- Definitions recovered after a fault are marked Recovered, because their nesting may have
  been misattributed.
- No partial bundle is left addressable at any point.
