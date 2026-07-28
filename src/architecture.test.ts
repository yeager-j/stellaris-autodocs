/**
 * Text gates over the frontend source tree.
 *
 * These are the TypeScript counterpart to `error.rs::no_source_file_makes_unexpected_serializable`,
 * and they are text gates for the same reason: the rules they enforce are about which module may
 * name a thing, which no type can express. Both rules below are stated in
 * `docs/technical-design.md` as prose, and prose is not a control.
 */

import { readdirSync, readFileSync, statSync } from "node:fs";
import { relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const SOURCE_ROOT = fileURLToPath(new URL(".", import.meta.url));
const REPOSITORY_ROOT = resolve(SOURCE_ROOT, "..");

/**
 * The only modules permitted to import `@tauri-apps/api`.
 *
 * "Direct `@tauri-apps/api` imports are confined to the Tauri documentation adapter and desktop
 * control implementation. Documentation features and shared UI modules must not import it"
 * (docs/technical-design.md, "Desktop control module"). The desktop control module does not exist
 * yet — Phase 10, task 6 — and its path is deliberately absent rather than reserved, because an
 * allow-list entry for a file nobody has written is an exemption nobody is using.
 */
const TAURI_API_IMPORTERS = ["src/documentation/tauri.ts"];

const TAURI_API = "@tauri-apps/api";
const RAW_HTML = "dangerouslySetInnerHTML";

type SourceFile = { path: string; text: string };

/**
 * This file is excluded from its own walk: it necessarily contains every literal it forbids, and
 * it is the gate rather than a module subject to it. The exclusion is one named path, so it
 * cannot quietly widen.
 */
const THIS_GATE = "src/architecture.test.ts";

function sourceFiles(directory: string = SOURCE_ROOT): SourceFile[] {
  return readdirSync(directory).flatMap((entry): SourceFile[] => {
    const absolute = resolve(directory, entry);
    if (statSync(absolute).isDirectory()) return sourceFiles(absolute);
    if (!/\.tsx?$/.test(entry)) return [];
    const path = relative(REPOSITORY_ROOT, absolute).replaceAll("\\", "/");
    if (path === THIS_GATE) return [];
    return [{ path, text: readFileSync(absolute, "utf8") }];
  });
}

/**
 * Pure so the rule can be aimed at a file that does not exist. A gate whose only input is the
 * repository can never be shown to detect anything.
 */
function importsTauriApiWithoutPermission(file: SourceFile): boolean {
  return file.text.includes(TAURI_API) && !TAURI_API_IMPORTERS.includes(file.path);
}

describe("the Tauri API import boundary", () => {
  it("is respected by every module outside the transport adapters", () => {
    const offenders = sourceFiles()
      .filter(importsTauriApiWithoutPermission)
      .map((file) => file.path);

    expect(offenders).toEqual([]);
  });

  it("names only modules that exist", () => {
    // Anti-vacuity. Renaming the adapter without updating this list would leave a gate that
    // permits nothing and therefore forbids nothing, and it would stay green while doing it.
    const present = new Set(sourceFiles().map((file) => file.path));

    expect(TAURI_API_IMPORTERS.filter((path) => !present.has(path))).toEqual([]);
  });

  it("rejects an unpermitted importer", () => {
    // The negative control. Without it, the first test above is equally green when the rule is
    // broken and when the walk silently returns nothing.
    const offender = {
      path: "src/routes/index.tsx",
      text: `import { invoke } from "${TAURI_API}/core";`,
    };
    const permitted = {
      path: TAURI_API_IMPORTERS[0],
      text: `import { invoke } from "${TAURI_API}/core";`,
    };

    expect(importsTauriApiWithoutPermission(offender)).toBe(true);
    expect(importsTauriApiWithoutPermission(permitted)).toBe(false);
  });
});

describe("source-derived rendering", () => {
  it("never reaches for raw HTML", () => {
    // The CSP release gate's first bullet: "No source-derived value is rendered as raw HTML, and
    // source-derived components do not use `dangerouslySetInnerHTML`" (docs/technical-design.md,
    // "Companion same-origin policy"). Enforced now, while the surface is small enough that the
    // rule is free; by Phase 10 it would be an audit.
    const offenders = sourceFiles()
      .filter((file) => file.text.includes(RAW_HTML))
      .map((file) => file.path);

    expect(offenders).toEqual([]);
  });
});
