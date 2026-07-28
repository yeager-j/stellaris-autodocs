import { describe, expect, it } from "vitest";

import {
  decodeEnvelope,
  DocumentationTransportError,
  HostRejectedError,
  rejectionToError,
  TransportContractError,
} from "./envelope";

describe("decodeEnvelope", () => {
  it("reads a success as the value branch", () => {
    expect(decodeEnvelope({ ok: true, value: { entries: [] } })).toStrictEqual({
      ok: true,
      value: { entries: [] },
    });
  });

  it("reads a refusal as the error branch", () => {
    expect(decodeEnvelope({ ok: false, error: { reason: "noPublishedRevision" } })).toStrictEqual({
      ok: false,
      error: { reason: "noPublishedRevision" },
    });
  });

  it("accepts a void success, whose value is null rather than absent", () => {
    // The design names the hazard directly: "a void success uses `null` or an explicit object
    // rather than JavaScript `undefined`, whose property would disappear during JSON encoding".
    // Rust's hand-written Serialize never applies `skip_serializing_if`, so the key is always
    // there — and this decoder tests presence, not truthiness.
    expect(decodeEnvelope({ ok: true, value: null })).toStrictEqual({ ok: true, value: null });
  });

  it("ignores properties it does not know", () => {
    // Deliberate. "Evolve contracts without requiring synchronized deployment: expand before
    // contracting." `value` and `error` are the discriminant's payload and can never be additive;
    // anything else may be, and rejecting it would make an additive host change a breaking one.
    expect(decodeEnvelope({ ok: true, value: 1, issuedAt: "later" })).toStrictEqual({
      ok: true,
      value: 1,
    });
  });

  it("does not look inside the payload", () => {
    // "The decoder does not recursively validate every success and error payload against a
    // manually duplicated TypeScript schema" (docs/technical-design.md, "Frontend response
    // decoding"). A payload of the wrong shape is a contract-suite failure, not a runtime throw.
    expect(decodeEnvelope({ ok: true, value: "not an entry list" })).toStrictEqual({
      ok: true,
      value: "not an entry list",
    });
  });

  describe("rejects a malformed envelope", () => {
    const malformed: [name: string, raw: unknown][] = [
      ["null", null],
      ["undefined", undefined],
      ["an array", []],
      ["a string", '{"ok":true,"value":1}'],
      ["a number", 42],
      ["an object with no discriminant", {}],
      ["a string discriminant", { ok: "true", value: 1 }],
      ["a numeric discriminant", { ok: 1, value: 1 }],
      ["a success with no value", { ok: true }],
      ["a refusal with no error", { ok: false }],
      ["a success carrying an error", { ok: true, error: { reason: "revisionMissing" } }],
      ["a refusal carrying a value", { ok: false, value: 1 }],
      ["both payloads at once", { ok: true, value: 1, error: {} }],
    ];

    it.each(malformed)("%s", (_name, raw) => {
      expect(() => decodeEnvelope(raw)).toThrow(TransportContractError);
    });
  });
});

describe("rejectionToError", () => {
  it("recognizes the host's own rejection and keeps its correlation identifier", () => {
    const error = rejectionToError({
      kind: "unexpected",
      correlationId: "0123456789abcdef0123456789abcdef",
    });

    expect(error).toBeInstanceOf(HostRejectedError);
    expect((error as HostRejectedError).correlationId).toBe("0123456789abcdef0123456789abcdef");
  });

  it("treats a framework rejection as a contract failure rather than an application outcome", () => {
    // Tauri rejects a malformed invocation with a bare JSON string. `Rejection` carries a fixed
    // `kind` precisely so the frontend can tell ours apart from the framework's by discriminant
    // rather than by shape (src-tauri/src/transport/envelope.rs). Neither becomes a Result: "React
    // does not infer an expected application outcome from an HTTP status or caught invocation
    // rejection" (D-070).
    expect(
      rejectionToError("invalid args `installation` for command `get_entry_list`")
    ).toBeInstanceOf(TransportContractError);
  });

  it("keeps the original rejection as the cause without rendering it", () => {
    const raw = { kind: "somethingElse" };

    expect(rejectionToError(raw).cause).toBe(raw);
  });

  it("produces errors a route boundary can catch as one kind", () => {
    expect(rejectionToError("anything")).toBeInstanceOf(DocumentationTransportError);
    expect(rejectionToError({ kind: "unexpected", correlationId: "ab" })).toBeInstanceOf(
      DocumentationTransportError
    );
  });
});
