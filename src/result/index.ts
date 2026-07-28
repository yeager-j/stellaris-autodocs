/**
 * The serializable Result contract, TypeScript side.
 *
 * This type is deliberately identical to the shape Rust puts on the wire
 * (`src-tauri/src/transport/envelope.rs`):
 *
 * ```text
 * { "ok": true,  "value": ... }
 * { "ok": false, "error": ... }
 * ```
 *
 * Because the in-memory type *is* the wire shape, decoding an envelope is a parse rather than a
 * transform — `decodeEnvelope` establishes the discriminant and hands back the same object. A
 * class-based Result (neverthrow and friends) would need a wrapping step whose only product is
 * combinators this phase has no use for.
 *
 * # Vendored, and why
 *
 * docs/decision-log.md, P-004 adopts the maintainer's extracted Result package, provisionally:
 * until it is published under an MIT-compatible name and pinned through this repository's
 * lockfile, "the application vendors the equivalent two-variant type and minimal utilities locally
 * behind the same import boundary". This module is that boundary — everything imports `@/result`,
 * so the swap is one file. The requirement the swap must satisfy is that it leaves the transport
 * contract suite (`src/documentation/contract.test.ts` and its Rust counterpart) unchanged.
 *
 * Minimal means minimal: two constructors and no combinators. `map`, `andThen`, and `unwrapOr`
 * arrive when a call site needs one, not in anticipation of it.
 */

export type Result<T, E> =
  { readonly ok: true; readonly value: T } | { readonly ok: false; readonly error: E };

export const ok = <T>(value: T): Result<T, never> => ({ ok: true, value });

export const err = <E>(error: E): Result<never, E> => ({ ok: false, error });
