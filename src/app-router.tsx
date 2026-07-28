import { createRouter } from "@tanstack/react-router";

import type { RouterContext } from "@/router-context";

import { routeTree } from "./routeTree.gen";

/**
 * Browser-history routing on the packaged custom scheme is **verified, not assumed**.
 *
 * D-076 rejects hash history, and Phase 11's Companion HTTP surface depends on real paths — but on
 * macOS and Linux the packaged app loads from `tauri://localhost`, and WebKit has historically
 * refused `history.pushState` on a non-http(s) origin (wry#170). Measured in a `tauri build --debug`
 * run on macOS with Tauri 2.11.5: origin `tauri://localhost`, `pushState` and `replaceState` both
 * succeed. If a future WebView regresses this, the answer is a design conversation, not a quiet
 * switch to hash history.
 */
export function createAppRouter(context: RouterContext) {
  return createRouter({
    routeTree,
    context,

    // D-077: the router is "explicitly configured with an application-chosen finite `gcTime` and
    // immediate or short `staleTime`; the design does not rely on TanStack Router's default
    // 30-minute garbage-collection window". A published revision is immutable, but *which* one is
    // published changes under the app's feet when a build completes, so a loader result is stale
    // the moment it is delivered and worth keeping only long enough to make going Back cheap.
    defaultStaleTime: 0,
    defaultGcTime: 30_000,

    // Preloading is off, so `defaultPreloadStaleTime` and `defaultPreloadGcTime` are left unset:
    // configuring windows for a mechanism that never runs would be a control that cannot act.
    defaultPreload: false,
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createAppRouter>;
  }
}
