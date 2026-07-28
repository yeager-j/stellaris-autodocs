import type { DocumentationClient } from "@/documentation/client";

/**
 * What every route loader is handed.
 *
 * The documentation client travels as router context rather than React context because loaders
 * run outside the component tree, and a loader reaching for a module-level singleton would make
 * "runtime bootstrap chooses one implementation for the application session"
 * (docs/technical-design.md, "Documentation client module") an unenforceable claim. Passing it in
 * at `createRouter` is what makes the choice happen exactly once, in `src/main.tsx`.
 */
export type RouterContext = {
  documentation: DocumentationClient;
};
