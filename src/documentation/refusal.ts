import type { ReadRefusal } from "./client";

/**
 * What each expected read outcome says to a person.
 *
 * Exhaustive by construction: the `never` assignment in the default arm makes a variant added to
 * `ReadRefusal` — which happens when one is added to Rust's `transport::tauri::ReadRefusal` and
 * mirrored here — a TypeScript compile error rather than a blank message. That is the same
 * discipline the Rust projection uses, where the match has no `_` arm for the same reason.
 *
 * The wording describes what happened to the *documentation*, not to a bundle, a path, or a
 * revision identifier: the documentation-client interface is forbidden those, and so is anything
 * rendered from it.
 */
export function refusalMessage(refusal: ReadRefusal): string {
  switch (refusal.reason) {
    case "noPublishedRevision":
      return "No documentation has been built for this mod yet.";
    case "referenceUnreadable":
      return "The record of what was last published could not be read.";
    case "revisionMissing":
      return "The published documentation is no longer where it was recorded.";
    case "revisionFromAnotherBuild":
      return `The published documentation was written by another version of this app (format ${refusal.found}, this build reads ${refusal.supported}). Rebuild it.`;
    case "revisionDamaged":
      return "The published documentation failed its integrity check. Rebuild it.";
    case "revisionDisplaced":
      return "The published documentation was replaced while it was being read. Try again.";
    case "revisionCarriesNoEntryList":
      return "The published documentation contains no entry list.";
    case "documentUnreadable":
      return "The entry list could not be read from the published documentation.";
    case "documentChanged":
      return "The entry list changed on disk while it was being read. Try again.";
    case "documentUndecodable":
      return "The entry list could not be decoded. Rebuild the documentation.";
    default: {
      const unhandled: never = refusal;
      return unhandled;
    }
  }
}
