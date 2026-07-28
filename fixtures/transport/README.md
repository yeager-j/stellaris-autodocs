# Transport contract vectors

The cross-language contract suite the design requires: "A cross-language contract suite covers every
documentation-client operation, every success shape, and every member of each operation-specific
expected-error union… It also includes negative controls for missing discriminants, incompatible
shapes, and non-JSON-safe assumptions" (`docs/technical-design.md`, "Serializable result contract").

## These files are the authority

Both languages compare against them; **neither generates them**. That is the whole point.
`src-tauri/src/transport/envelope.rs` has no `Deserialize` on purpose — "a round-trip test can be
green with both halves wrong" — so the two implementations are Rust's `Serialize` and the
TypeScript decoder in `src/documentation/envelope.ts`, and these files are the third thing that
neither of them can silently agree with. A generator would collapse that back into one authority.

Who reads what:

| directory | Rust (`transport::contract_vectors`) | TypeScript (`src/documentation/contract.test.ts`) |
| --- | --- | --- |
| `get_entry_list/` | serializes each DTO and compares bytes | decodes each and compares the decoded value |
| `rejection/` | serializes `Rejection` and compares bytes | feeds it to a rejecting `invoke` and expects `HostRejectedError` |
| `malformed/` | — | expects each to throw `TransportContractError` |

`malformed/` has no Rust side because Rust cannot produce those shapes; they exist to prove the
TypeScript decoder rejects what the contract forbids rather than accepting whatever it is handed.

## Format

**Compact, one line, no trailing newline.** The comparison is on bytes, not on a parsed value, so
the file pins key *order* too — `ok` before its payload, which `envelope.rs` asserts on the string
for the same reason. A `serde_json::Value` comparison would discard that silently.

## Adding a variant

A new member of a wire union needs a file here in the same commit. Both suites check completeness
rather than trusting that: the Rust side builds its table from an exhaustive `match` with no `_`
arm and compares the file count, and the TypeScript side asserts every file in each directory was
consumed. Adding the Rust variant without a fixture fails to compile; adding a fixture without
wiring it fails the count.
