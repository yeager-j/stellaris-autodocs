# A mod descriptor, written in the lexical style descriptors actually use.
#
# Discriminates: does the parser cope with `key="value"` — no spaces around the operator —
# and `tags={` with no space before the brace? Descriptors are a distinct dialect from the
# tab-indented `key = value` of script files, and the resolver needs them: `replace_path` is
# declared here and nowhere else, and it excludes every other source's files in the named
# directory (`docs/spikes/resolver-evaluation.md:218`).
#
# Comments in a real descriptor are unusual; they are here because every fixture in this
# repository explains itself.
#
# Expected: five top-level definitions; `tags` an array of two quoted strings.
name="Stellaris Docs Parser Fixture"
version="1.0"
supported_version="v4.4.*"
replace_path="common/technology"
tags={
	"Utilities"
	"Fixes"
}
