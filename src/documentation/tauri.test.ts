import { describe, expect, it, vi } from "vitest";

import { HostRejectedError, TransportContractError } from "./envelope";
import { tauriDocumentationClient, type Invoke } from "./tauri";

const INSTALLATION = "ab".repeat(32);

const ENTRY_LIST = {
  installation: INSTALLATION,
  entries: [{ category: "technology", identifier: "tech_skeleton", displayName: null }],
};

function clientThatResolves(value: unknown) {
  const invoke = vi.fn<Invoke>().mockResolvedValue(value);
  return { client: tauriDocumentationClient(invoke), invoke };
}

function clientThatRejects(reason: unknown) {
  const invoke = vi.fn<Invoke>().mockRejectedValue(reason);
  return { client: tauriDocumentationClient(invoke), invoke };
}

describe("the Tauri documentation adapter", () => {
  it("invokes the read command with the argument name Tauri derives", () => {
    // Rust's parameter is `installation: ModInstallationId`; Tauri camelCases command parameters,
    // and this one is a single word, so the key is `installation`. Pinned because a rename on
    // either side is a silent malformed invocation rather than a type error.
    const { client, invoke } = clientThatResolves({ ok: true, value: ENTRY_LIST });

    void client.getEntryList(INSTALLATION);

    expect(invoke).toHaveBeenCalledWith("get_entry_list", { installation: INSTALLATION });
  });

  it("returns a decoded success", async () => {
    const { client } = clientThatResolves({ ok: true, value: ENTRY_LIST });

    await expect(client.getEntryList(INSTALLATION)).resolves.toStrictEqual({
      ok: true,
      value: ENTRY_LIST,
    });
  });

  it("returns a refusal as a value rather than throwing", async () => {
    // A refusal is a successful operation with an expected outcome. Throwing here would route it
    // to an error boundary, which is exactly the confusion D-070 forbids.
    const { client } = clientThatResolves({ ok: false, error: { reason: "noPublishedRevision" } });

    await expect(client.getEntryList(INSTALLATION)).resolves.toStrictEqual({
      ok: false,
      error: { reason: "noPublishedRevision" },
    });
  });

  it("throws when the host rejects, carrying the correlation identifier", async () => {
    const { client } = clientThatRejects({
      kind: "unexpected",
      correlationId: "0123456789abcdef0123456789abcdef",
    });

    await expect(client.getEntryList(INSTALLATION)).rejects.toThrow(HostRejectedError);
  });

  it("throws when the framework rejects a malformed invocation", async () => {
    const { client } = clientThatRejects("invalid args for command `get_entry_list`");

    await expect(client.getEntryList(INSTALLATION)).rejects.toThrow(TransportContractError);
  });

  it("throws when the resolved payload is not an envelope", async () => {
    const { client } = clientThatResolves({ entries: [] });

    await expect(client.getEntryList(INSTALLATION)).rejects.toThrow(TransportContractError);
  });
});
