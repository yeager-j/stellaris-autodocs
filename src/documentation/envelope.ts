/**
 * The TypeScript half of the serializable Result contract.
 *
 * `src-tauri/src/transport/envelope.rs` deliberately has no `Deserialize`: "A Rust decoder here
 * would be a second statement of the wire shape inside the crate that produces it, and a
 * round-trip test can be green with both halves wrong. The real second implementation is the
 * TypeScript documentation client's decoder; the cross-language suite is what compares them."
 * This module is that second implementation, and `./contract.test.ts` is that suite.
 *
 * # Two channels, and they never mix
 *
 * A Tauri command *resolves* with an envelope for every expected application outcome and *rejects*
 * only when it could not validly execute — malformed invocation, framework failure, or an
 * unexpected internal failure. So a rejection is never turned into a `Result` error value
 * (docs/decision-log.md, D-070); it becomes a thrown `DocumentationTransportError` for a route
 * boundary to present.
 */

import { err, ok, type Result } from "@/result";

/** Anything that went wrong at the transport rather than in the application. */
export class DocumentationTransportError extends Error {}

/** The host did not speak the contract, or the framework refused the call. */
export class TransportContractError extends DocumentationTransportError {
  constructor(detail: string, options?: ErrorOptions) {
    super(`the host response did not satisfy the transport contract: ${detail}`, options);
    this.name = "TransportContractError";
  }
}

/**
 * The host reported an unexpected internal failure. `correlationId` is the only thing it carries —
 * detailed error chains stay in the desktop log — and it is what a person reads back to support.
 */
export class HostRejectedError extends DocumentationTransportError {
  readonly correlationId: string;

  constructor(correlationId: string, options?: ErrorOptions) {
    super(`the host rejected the request [${correlationId}]`, options);
    this.name = "HostRejectedError";
    this.correlationId = correlationId;
  }
}

function isPlainObject(raw: unknown): raw is Record<string, unknown> {
  return typeof raw === "object" && raw !== null && !Array.isArray(raw);
}

/**
 * Establishes that `raw` is a well-formed envelope and returns it as a `Result`.
 *
 * The rule is exactly the design's: "The decoder requires a plain object with a boolean `ok`
 * discriminant and exactly the corresponding `value` or `error` property. A malformed envelope
 * throws a transport-contract error." Presence is tested with `in` rather than
 * `!== undefined`, because `{"ok":true,"value":null}` is a valid void success and the *key* is
 * what the contract fixes.
 *
 * It does not recursively validate the payload, and it does not reject unknown sibling properties
 * — see the tests, where both choices are stated with their reasons. The payload cast is
 * unchecked; the evidence that it holds is the cross-language fixture suite plus the fact that the
 * same Rust host ships this frontend and serializes its responses.
 */
export function decodeEnvelope<T, E>(raw: unknown): Result<T, E> {
  if (!isPlainObject(raw)) {
    throw new TransportContractError(`expected a plain object, received ${describe(raw)}`);
  }
  if (typeof raw.ok !== "boolean") {
    throw new TransportContractError(`expected a boolean \`ok\`, received ${describe(raw.ok)}`);
  }

  const [expected, forbidden] = raw.ok
    ? (["value", "error"] as const)
    : (["error", "value"] as const);
  if (!(expected in raw)) {
    throw new TransportContractError(`\`ok\` is ${raw.ok} but \`${expected}\` is absent`);
  }
  if (forbidden in raw) {
    throw new TransportContractError(`\`ok\` is ${raw.ok} but \`${forbidden}\` is present`);
  }

  return raw.ok ? ok(raw.value as T) : err(raw.error as E);
}

/**
 * Turns a rejected `invoke` into the error a route boundary will present.
 *
 * `Rejection` carries a fixed `kind` so this function can tell the host's own rejection from the
 * framework's by discriminant rather than by shape — Tauri rejects a malformed invocation with a
 * bare JSON string. The raw rejection is kept as `cause` for devtools and is never rendered:
 * "React has route-level and application-level error boundaries that present the correlation
 * identifier without exposing internal chains."
 */
export function rejectionToError(rejection: unknown): DocumentationTransportError {
  if (
    isPlainObject(rejection) &&
    rejection.kind === "unexpected" &&
    typeof rejection.correlationId === "string"
  ) {
    return new HostRejectedError(rejection.correlationId, { cause: rejection });
  }
  return new TransportContractError(`the command rejected with ${describe(rejection)}`, {
    cause: rejection,
  });
}

/** A type name for a message. Never the value itself, which may be host detail. */
function describe(raw: unknown): string {
  if (raw === null) return "null";
  if (Array.isArray(raw)) return "an array";
  return `a ${typeof raw}`;
}
