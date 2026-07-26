# Resolve content before generating documentation

The MVP will document one selected Target Mod against Vanilla Content and may expose cross-mod references as unresolved. Parsing and content resolution will produce a provenance-preserving resolved content model before documentation generation, so the documentation generator does not need to know whether its input came from one mod or, in a later feature, a complete Playset.

Vanilla Content and the Target Mod are two source contributors, not universally ordered layers. Common exact-path shadowing and `replace_path` selection happen first. The resolver then constructs a content-family-specific semantic stream: script registries and sprites use global logical-path order across surviving files, while localization uses its Vanilla, enabled-mod, and `replace/` phases. Registry-specific first- or last-registration behavior applies only within that stream.

DLC-gated definitions live in the base-game file set. DLC ownership is retained as a requirement such as `host_has_dlc`, not modeled as a separate definition source or precedence layer.

Playset support will extend the resolver rather than introduce a separate documentation path. It must preserve the origins and override history of definitions instead of flattening the Playset into merged source files.
