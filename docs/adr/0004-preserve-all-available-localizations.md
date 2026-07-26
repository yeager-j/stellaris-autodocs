# Preserve all available localizations

The content model will preserve every localization supplied by Stellaris, locally available DLC, and the Target Mod rather than resolving text permanently to English during ingestion. Player Documentation will default to the player's configured Stellaris language, fall back to English, and finally show the raw localization key or script identifier when no suitable localization exists.

This keeps the deterministic documentation pipeline language-independent while allowing incomplete mod translations to degrade visibly instead of losing content.

The MVP renderer will translate known Stellaris color and style markers into controlled CSS, recursively resolve Static Localization References with cycle detection, and render known inline icons. Runtime Localization Tokens, concept links, and unknown constructs remain visibly raw rather than being guessed or discarded.

Interactive Concept Links are a post-MVP enhancement. They may later open linked documentation in a popover without changing the underlying preserved localization.
