/**
 * The TypeScript half of the cross-language contract suite.
 *
 * The committed files under `fixtures/transport/` are the authority. Rust compares what it
 * serializes against them (`src-tauri/src/transport/contract_vectors.rs`); this compares what the
 * decoder makes of them. Neither generates them — see that directory's README.
 *
 * Every directory is checked for *completeness*, not just for the cases named here: a fixture
 * nobody reads is a fixture that looks like coverage while providing none, which is the same
 * failure mode as a gate that has always been green.
 */

import { readdirSync, readFileSync } from "node:fs";
import { basename, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it, vi } from "vitest";

import type { EntryListView, ReadRefusal } from "./client";
import { decodeEnvelope, HostRejectedError, TransportContractError } from "./envelope";
import { tauriDocumentationClient, type Invoke } from "./tauri";

const VECTORS = fileURLToPath(new URL("../../fixtures/transport", import.meta.url));

const INSTALLATION = "ab".repeat(32);

function vectorNames(directory: string): string[] {
  return readdirSync(resolve(VECTORS, directory))
    .filter((name) => name.endsWith(".json"))
    .sort();
}

function vector(directory: string, name: string): unknown {
  return JSON.parse(readFileSync(resolve(VECTORS, directory, name), "utf8"));
}

describe("get_entry_list vectors", () => {
  const consumed = new Set<string>();

  function decode(name: string) {
    consumed.add(name);
    return decodeEnvelope<EntryListView, ReadRefusal>(vector("get_entry_list", name));
  }

  it("decodes a populated entry list", () => {
    expect(decode("success-two-entries.json")).toStrictEqual({
      ok: true,
      value: {
        installation: INSTALLATION,
        entries: [
          {
            category: "technology",
            identifier: "tech_skeleton",
            displayName: "Skeleton Technology",
          },
          // `displayName: null` is present, not absent. An entry with no resolved localized name
          // is a different fact from one resolved to the empty string, and this is where the two
          // implementations would drift apart unnoticed.
          { category: "technology", identifier: "tech_unnamed", displayName: null },
        ],
      },
    });
  });

  it("decodes a revision that documents nothing as a success", () => {
    // `entries: []` is a success; `revisionCarriesNoEntryList` below is a refusal. Empty and
    // absent must not collapse into one another.
    expect(decode("success-empty.json")).toStrictEqual({
      ok: true,
      value: { installation: INSTALLATION, entries: [] },
    });
  });

  const refusals: [file: string, expected: ReadRefusal][] = [
    ["refusal-noPublishedRevision.json", { reason: "noPublishedRevision" }],
    ["refusal-referenceUnreadable.json", { reason: "referenceUnreadable" }],
    ["refusal-revisionMissing.json", { reason: "revisionMissing" }],
    [
      "refusal-revisionFromAnotherBuild.json",
      { reason: "revisionFromAnotherBuild", found: 7, supported: 1 },
    ],
    ["refusal-revisionDamaged.json", { reason: "revisionDamaged" }],
    ["refusal-revisionDisplaced.json", { reason: "revisionDisplaced" }],
    ["refusal-revisionCarriesNoEntryList.json", { reason: "revisionCarriesNoEntryList" }],
    ["refusal-documentUnreadable.json", { reason: "documentUnreadable" }],
    ["refusal-documentChanged.json", { reason: "documentChanged" }],
    ["refusal-documentUndecodable.json", { reason: "documentUndecodable" }],
  ];

  it.each(refusals)("decodes %s", (file, expected) => {
    expect(decode(file)).toStrictEqual({ ok: false, error: expected });
  });

  it("reads every vector in the directory", () => {
    expect([...consumed].sort()).toStrictEqual(vectorNames("get_entry_list"));
  });
});

describe("malformed vectors", () => {
  const names = vectorNames("malformed");

  it("has vectors to check", () => {
    // Anti-vacuity: `it.each([])` passes silently, so an empty or mistyped directory would make
    // the whole suite below green without running a single case.
    expect(names.length).toBeGreaterThan(0);
  });

  it.each(names)("rejects %s as a contract failure", (name) => {
    expect(() => decodeEnvelope(vector("malformed", name))).toThrow(TransportContractError);
  });
});

describe("rejection vectors", () => {
  it("turns the host's rejection into a thrown error carrying its correlation identifier", async () => {
    const rejection = vector("rejection", "unexpected.json") as { correlationId: string };
    const invoke = vi.fn<Invoke>().mockRejectedValue(rejection);

    const failure = await tauriDocumentationClient(invoke)
      .getEntryList(INSTALLATION)
      .catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(HostRejectedError);
    expect((failure as HostRejectedError).correlationId).toBe(rejection.correlationId);
  });

  it("reads every vector in the directory", () => {
    expect(vectorNames("rejection").map((name) => basename(name))).toStrictEqual([
      "unexpected.json",
    ]);
  });
});
