import { RouterProvider } from "@tanstack/react-router";
import React from "react";
import ReactDOM from "react-dom/client";

import { createAppRouter } from "@/app-router";
import { tauriDocumentationClient } from "@/documentation/tauri";

import "@/index.css";

/**
 * Runtime bootstrap.
 *
 * This is where the application session's documentation client is chosen, once, and handed to the
 * router as context. Phase 11 adds the Companion HTTP adapter and this becomes a real choice; the
 * point of making it here even while there is only one implementation is that pages and loaders
 * never acquire a reason to ask which transport they are on
 * (docs/technical-design.md, "Documentation client module"; D-050).
 */
const router = createAppRouter({ documentation: tauriDocumentationClient() });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <RouterProvider router={router} />
  </React.StrictMode>
);
