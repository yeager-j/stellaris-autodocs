# Stellaris Mod Documentation

This context describes a local documentation experience derived from installed Stellaris mods and shared with nearby devices.

## Language

**Mod Source**:
The locally installed files that define a Stellaris mod.
_Avoid_: Upload, mod package

**Target Mod**:
The installed mod currently selected for Player Documentation.
_Avoid_: Active mod, source mod

**Searchable Entry**:
A localized content record that can appear in search even when its content category does not yet have full Player Documentation.
_Avoid_: Search result, documented object

**Entry Key**:
The stable pair of content category and raw Stellaris script identifier that addresses one effective Searchable Entry within a Target Mod's Documentation Revision. Redefinitions of the same category and identifier share one Entry Key.
_Avoid_: Localized name, localization key, document ID

**Stellaris Installation**:
A local installation of Stellaris that supplies Vanilla Content and its visual assets.
_Avoid_: Game folder, vanilla mod

**Mod Library**:
The unified catalog of installed mods discovered across one or more Discovery Locations and available for selection as a Target Mod.
_Avoid_: Playset, mod folder

**Discovery Location**:
A user-confirmed filesystem location the app scans for Stellaris or Mod Installations. Its stable identity represents the configured location independently of its current absolute path.
_Avoid_: Mod Library, Mod Installation, scan result

**Mod Installation**:
One physical copy of a mod at a particular source location. Multiple Mod Installations may share a title, Workshop identity, or declared version while containing different source.
_Avoid_: Mod identity, duplicate

**Declared Dependency**:
A dependency named by a mod's metadata. It is advisory and does not cause another mod to be composed into the Target Mod during the MVP.
_Avoid_: Active dependency, Playset member

**Vanilla Content**:
The base-game and locally available DLC content distributed with Stellaris that provides the shared baseline referenced or changed by a Target Mod.
_Avoid_: Base mod, built-in mod

**Playset**:
An ordered collection of enabled mods that together alter a Stellaris game.
_Avoid_: Mod folder, mod list

**Unresolved Reference**:
A reference whose definition cannot be found in the Target Mod or its available supporting content.
_Avoid_: Broken link, missing documentation

**Analysis Issue**:
A source location the app could not parse or interpret completely and that may make some Player Documentation incomplete.
_Avoid_: Mod error, ignored file

**Source Excerpt**:
A bounded, read-only view of the original Mod Source around a documented definition or fact, including any comments present in that range.
_Avoid_: Generated documentation, full-file view

**Resolved Base Value**:
The static value obtained by following a scripted constant such as `@acot_tier7cost3`, before difficulty, empire, saved-game, or other runtime modifiers.
_Avoid_: Final value, player cost

**Static Localization Reference**:
A `$KEY$` reference whose replacement text can be determined solely from the resolved localization files.
_Avoid_: Runtime substitution, scripted localization

**Runtime Localization Token**:
A localization expression such as `[Root.GetName]` whose value requires live game scope or saved-game state.
_Avoid_: Missing localization, unresolved key

**Concept Link**:
A localization construct that refers to another game concept and could later open that concept's documentation in a link or popover.
_Avoid_: Static localization reference, source link

**Incomplete Documentation**:
Player Documentation generated from all successfully analyzed content while one or more relevant Analysis Issues remain visibly disclosed.
_Avoid_: Failed documentation, best guess

**Documentation Revision**:
An immutable, published result of one completed analysis against exact source fingerprints and analysis-component versions. It may contain Incomplete Documentation when the completed analysis reported Analysis Issues.
_Avoid_: Live documentation, partial build, cache entry

**Unlockable**:
Player-facing content that becomes available only after particular requirements, triggers, choices, or effects.
_Avoid_: Object, item

**Unlock Path**:
A distinct, traceable route through requirements, player-facing actions or occurrences, triggers, choices, and effects that can make an Unlockable available, independent of any particular saved game. An Unlockable may have multiple Unlock Paths.
_Avoid_: Dependency list, source references, next step

**Hidden Route**:
An Unlock Path the user has chosen to omit from the normal page presentation without removing it from generated knowledge.
_Avoid_: Deleted route, ignored source

**Route Summary**:
A concise sequence of Player-Facing Anchors and required selections that tells a player how to follow an Unlock Path without reproducing every internal mechanic.
_Avoid_: Simulation, event dump

**Unlock Effect**:
The precise result a route has on an Unlockable, such as adding a research option, adding research progress, completing a technology, or changing its Draw Weight.
_Avoid_: Unlock, reward

**Grant Site**:
A source action that applies one or more Unlock Effects to the same Unlockable. In the MVP, each Grant Site produces one route card and co-located effects are combined.
_Avoid_: Route, search result

**Event Chain**:
A branching sequence of related events experienced by a player, often culminating in one or more Unlockables.
_Avoid_: Event file, event list

**Player-Facing Anchor**:
A localized or meaningfully describable action or occurrence that a player can recognize in the game and that advances an Unlock Path. Event options, special projects, decisions, anomalies, archaeology stages, diplomatic actions, research choices, and construction actions are typical anchors; internal flags, variables, and bookkeeping events are not.
_Avoid_: Event node, user action

**Entry Point**:
The Player-Facing Anchor at which an Unlock Path or one of its linked sections begins. It is local to that explanation rather than necessarily the earliest causal event in the game.
_Avoid_: First event, root node, game-state checkpoint

**Draw Weight**:
The relative value used when selecting a technology from the eligible research pool; it is not an absolute probability.
_Avoid_: Drop rate, research chance

**Drawable**:
An eligible technology whose effective Draw Weight is greater than zero and can therefore appear through normal random research selection.
_Avoid_: Available, listed

**Weight Modifier**:
A conditional multiplier applied to a Draw Weight. A zero multiplier prevents normal random selection while it applies but is not an eligibility requirement.
_Avoid_: Requirement, blocker

**Player Documentation**:
An explanation of a mod's concepts, requirements, effects, choices, progression, and relationships for someone playing the mod.
_Avoid_: Developer documentation, source reference

**Documentation Export**:
A portable rendering of Player Documentation for publication outside the app.
_Avoid_: Generated documentation, wiki sync

**Desktop Host**:
The computer that has access to the Mod Source and makes its documentation available.
_Avoid_: Server, main device

**Companion Device**:
Another device on the Desktop Host's local network that can read the same documentation experience without directly accessing the Mod Source or changing Desktop Host configuration.
_Avoid_: Phone, mobile app

**Companion Session**:
Temporary authorization for one browser to read Player Documentation from the Desktop Host after opening its QR-code link. It expires when Companion Mode is disabled or the desktop app exits.
_Avoid_: Account, login, registered device

**Companion-Ready Cache**:
A complete documentation cache whose published manifest matched Mod Source, Vanilla Content, analysis versions, and referenced browser-safe asset inputs at the host's latest required verification event. A Companion Device may read it without triggering analysis, source hashing, or filesystem writes.
_Avoid_: Existing cache, active mod

**Companion Mode**:
The explicit, normally disabled state in which a Desktop Host permits authorized Companion Devices to access its documentation.
_Avoid_: Remote mode, mobile app
