# Preserve every available language for documented localization

The content model ingests localization supplied by Stellaris and the Target Mod, then preserves every available language for each localization key cited by generated documentation plus the transitive closure of its Static Localization References. It does not preserve unrelated localization keys merely because they were present in the source tables. Player Documentation defaults to the player's configured Stellaris language, falls back to English, and finally shows the raw localization key or script identifier when no suitable localization exists.

This keeps the deterministic documentation pipeline language-independent for everything the documentation can render while allowing incomplete mod translations to degrade visibly instead of losing content. Changing language does not reparse Mod Source or rebuild Player Documentation.

The narrower retained-key set was adopted after the [revision-bundle evaluation](../spikes/revision-bundle-evaluation.md) found that generated documentation reaches 1.15% to 1.45% of the complete localization tables. Preserving every source key would store 151 to 178 MiB per revision for text no reader can address; preserving the cited-key closure in every language costs 1.74 to 2.59 MiB and produces observationally identical expanded documentation.

The MVP renderer will translate known Stellaris color and style markers into controlled CSS, recursively resolve Static Localization References with cycle detection, and render known inline icons. Runtime Localization Tokens, concept links, and unknown constructs remain visibly raw rather than being guessed or discarded.

Interactive Concept Links are a post-MVP enhancement. They may later open linked documentation in a popover without changing the underlying preserved localization.
