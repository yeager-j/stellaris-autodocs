# Resolve content before generating documentation

The MVP will document one selected Target Mod against Vanilla Content and may expose cross-mod references as unresolved. Parsing and content resolution will produce a provenance-preserving resolved content model before documentation generation, so the documentation generator does not need to know whether its input came from one mod or, in a later feature, a complete Playset.

Playset support will extend the resolver rather than introduce a separate documentation path. It must preserve the origins and override history of definitions instead of flattening the Playset into merged source files.
