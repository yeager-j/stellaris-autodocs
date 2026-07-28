import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, type FormEvent } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

export const Route = createFileRoute("/")({
  component: SkeletonConsole,
});

/**
 * The Phase 3 stand-in for the Mod Library.
 *
 * A Mod Installation identifier is a digest derived from a Discovery Location identifier and a mod
 * root, so nothing can guess one and there is no read command that lists them — discovery-backed
 * resolution is Phase 9. A `test-support` build prints the seeded identifier on startup
 * (`composition::candidate_source`), and this field is how a person gets it into the application.
 *
 * **Phase 10, task 2 deletes this page** and replaces it with the real Mod Library.
 */
function SkeletonConsole() {
  const navigate = useNavigate();
  const [installation, setInstallation] = useState("");

  function open(event: FormEvent) {
    event.preventDefault();
    const trimmed = installation.trim();
    if (trimmed === "") return;
    // No format check here. `ModInstallationId`'s `Deserialize` is the single authority for what a
    // well-formed identifier is, and a malformed one is meant to reach it: the command rejects,
    // and that rejection path is the only place the redaction seam is observable end to end.
    void navigate({ to: "/installations/$installation", params: { installation: trimmed } });
  }

  return (
    <section className="space-y-6">
      <div className="space-y-2">
        <h1 className="font-heading text-2xl font-bold">Skeleton console</h1>
        <p className="text-sm text-muted-foreground">
          Phase 3 scaffolding. Paste the Mod Installation identifier a <code>test-support</code>{" "}
          build prints on startup. The Mod Library replaces this page in Phase 10.
        </p>
      </div>

      <form onSubmit={open} className="space-y-3">
        <Label htmlFor="installation">Mod Installation identifier</Label>
        <Input
          id="installation"
          name="installation"
          value={installation}
          onChange={(event) => setInstallation(event.target.value)}
          placeholder="64 lowercase hex characters"
          autoComplete="off"
          spellCheck={false}
          className="font-mono"
        />
        <Button type="submit">Open entries</Button>
      </form>
    </section>
  );
}
