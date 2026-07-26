# Develop on macOS with a three-platform release target

The functional MVP will target macOS so the technology-documentation slice can be validated quickly. The first public release will also be packaged and tested on Windows and Linux using available real users and machines, so filesystem discovery, path handling, process management, and caching must remain behind platform-aware boundaries rather than becoming macOS-specific core behavior. The selected release artifacts are a macOS `.dmg`, a Windows NSIS installer, and Linux AppImage plus Debian `.deb`; other package formats make no compatibility claim until tested separately.

The macOS-first implementation must explicitly handle Documents-folder privacy permission because local Stellaris mods and game settings commonly live below the user's Documents directory. First launch explains the access request before scanning. Denial produces a visible unavailable-location state with path correction or folder selection and retry; it must not look like an empty mod library or delete prior configuration. If settings access prevents language detection, the app shows a non-blocking notice and falls back to the explicit app override or English.

Packaged macOS verification covers both granting and denying this access, in addition to the shared cross-platform behavior.
