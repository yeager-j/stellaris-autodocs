/**
 * The desktop documentation adapter: the documentation client implemented over Tauri commands.
 *
 * **This is one of the only two modules permitted to import `@tauri-apps/api`** — the other being
 * the desktop control implementation, which Phase 10 introduces. "Documentation features and
 * shared UI modules must not import it" (docs/technical-design.md, "Desktop control module").
 * `src/architecture.test.ts` enforces that rather than trusting this comment.
 *
 * The module holds no rules. It names a command, hands the framework its arguments, and passes
 * what comes back to the decoder. Anything here that decided what an outcome *means* would be a
 * second authority over behaviour the Rust `application` module owns — the same constraint
 * `src-tauri/src/transport/tauri.rs` states for its own side of the boundary.
 */

import { invoke as tauriInvoke } from "@tauri-apps/api/core";

import type { DocumentationClient, EntryListView, ReadRefusal } from "./client";
import { decodeEnvelope, rejectionToError } from "./envelope";

/**
 * The one function this adapter needs from the framework.
 *
 * It is a constructor parameter with a default rather than a module mock so that tests supply a
 * fake by ordinary means, and so the seam is visible in the type rather than hidden in a test
 * runner's module registry.
 */
export type Invoke = (command: string, args: Record<string, unknown>) => Promise<unknown>;

export function tauriDocumentationClient(invoke: Invoke = tauriInvoke): DocumentationClient {
  return {
    async getEntryList(installation) {
      return command<EntryListView, ReadRefusal>(invoke, "get_entry_list", { installation });
    },
  };
}

async function command<T, E>(invoke: Invoke, name: string, args: Record<string, unknown>) {
  // The rejection is converted before the decode is attempted, because the two failures are
  // different kinds: a rejection means the command could not validly execute, while a resolved
  // value that is not an envelope means the host broke the contract.
  const raw = await invoke(name, args).catch((rejection: unknown) => {
    throw rejectionToError(rejection);
  });
  return decodeEnvelope<T, E>(raw);
}
