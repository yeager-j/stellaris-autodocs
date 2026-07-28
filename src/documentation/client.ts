/**
 * The documentation client: the only module React pages use for documentation reads
 * (docs/technical-design.md, "Documentation client module").
 *
 * The interface hides runtime transport selection, request serialization, response parsing,
 * transport failures, and host error normalization. It has one implementation today — the desktop
 * adapter in `./tauri` — and a second is a named Phase 11 deliverable (the Companion HTTP adapter),
 * which is what earns the seam now rather than on speculation. Runtime bootstrap picks one for the
 * session in `src/main.tsx`; pages and loaders never branch on transport.
 *
 * # These types mirror Rust by hand, deliberately
 *
 * Every type below is the TypeScript statement of a `#[serde(rename_all = "camelCase")]` DTO in
 * `src-tauri/src/transport/tauri.rs`. There is no binding generator: "Application-owned TypeScript
 * DTOs and cross-language fixtures provide compile-time use and contract evidence. If payload
 * drift becomes a demonstrated problem, the preferred response is Rust-to-TypeScript contract
 * generation rather than a second hand-maintained schema authority"
 * (docs/technical-design.md, "Frontend response decoding"). The fixtures under
 * `fixtures/transport/` are what stop the mirror drifting silently.
 */

import type { Result } from "@/result";

/** One entry of a published revision. Mirrors `transport::tauri::EntryView`. */
export type EntryView = {
  category: string;
  identifier: string;
  /**
   * `null` means no localized name was resolved, which is not a resolved empty one. Rust
   * serializes it rather than skipping it, so the property is always present — never `undefined`.
   */
  displayName: string | null;
};

/** Mirrors `transport::tauri::EntryListView`. */
export type EntryListView = {
  installation: string;
  entries: EntryView[];
};

/**
 * Every expected outcome of the read, discriminated by `reason`. Mirrors
 * `transport::tauri::ReadRefusal`, which is `#[serde(tag = "reason")]`.
 *
 * A refusal is a *successful* operation with an expected outcome, not an error: it is rendered as
 * a page state, never thrown (docs/decision-log.md, D-070).
 */
export type ReadRefusal =
  | { reason: "noPublishedRevision" }
  | { reason: "referenceUnreadable" }
  | { reason: "revisionMissing" }
  | { reason: "revisionFromAnotherBuild"; found: number; supported: number }
  | { reason: "revisionDamaged" }
  | { reason: "revisionDisplaced" }
  | { reason: "revisionCarriesNoEntryList" }
  | { reason: "documentUnreadable" }
  | { reason: "documentChanged" }
  | { reason: "documentUndecodable" };

export interface DocumentationClient {
  /**
   * The entries of the revision currently published for one Mod Installation.
   *
   * `installation` is a plain string on purpose. `ModInstallationId`'s `Deserialize` is the single
   * authority for the 64-lowercase-hex format — a format only Rust mints — and restating it here
   * would be a second one. A malformed value is a malformed invocation: the command rejects rather
   * than resolving, and the adapter turns that into a thrown transport error.
   */
  getEntryList(installation: string): Promise<Result<EntryListView, ReadRefusal>>;
}
