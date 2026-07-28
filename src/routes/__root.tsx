import { createRootRouteWithContext, Link, Outlet } from "@tanstack/react-router";

import type { RouterContext } from "@/router-context";

export const Route = createRootRouteWithContext<RouterContext>()({
  component: RootLayout,
});

function RootLayout() {
  return (
    <div className="min-h-screen bg-background text-foreground">
      <header className="border-b">
        <nav className="mx-auto flex max-w-6xl items-baseline gap-4 px-6 py-4">
          <Link to="/" className="font-heading text-lg font-bold">
            Stellaris Docs
          </Link>
        </nav>
      </header>
      <main className="mx-auto max-w-6xl px-6 py-8">
        <Outlet />
      </main>
    </div>
  );
}
