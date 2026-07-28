import { createFileRoute } from "@tanstack/react-router";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { DocumentationTransportError, HostRejectedError } from "@/documentation/envelope";
import { refusalMessage } from "@/documentation/refusal";

export const Route = createFileRoute("/installations/$installation")({
  // The loader *returns* the Result rather than throwing on a refusal. A refusal is a successful
  // operation with an expected outcome; throwing would route it to `errorComponent`, which is
  // reserved for the transport failing (docs/decision-log.md, D-070).
  loader: ({ context, params }) => context.documentation.getEntryList(params.installation),
  component: InstallationEntries,
  errorComponent: TransportFailure,
});

function InstallationEntries() {
  const outcome = Route.useLoaderData();

  if (!outcome.ok) {
    return (
      <Alert>
        <AlertTitle>No entries to show</AlertTitle>
        <AlertDescription>{refusalMessage(outcome.error)}</AlertDescription>
      </Alert>
    );
  }

  const { entries } = outcome.value;

  return (
    <section className="space-y-6">
      <h1 className="font-heading text-2xl font-bold">Entries</h1>

      {entries.length === 0 ? (
        // An empty list is a success: this revision documents nothing. That is a different answer
        // from `revisionCarriesNoEntryList`, which is a refusal, and the two must not collapse.
        <p className="text-sm text-muted-foreground">
          This revision was published without any entries.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Category</TableHead>
              <TableHead>Identifier</TableHead>
              <TableHead>Name</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {entries.map((entry) => (
              <TableRow key={`${entry.category}/${entry.identifier}`}>
                <TableCell>{entry.category}</TableCell>
                <TableCell className="font-mono">{entry.identifier}</TableCell>
                <TableCell>
                  {entry.displayName ?? (
                    // Absent is not empty: no localized name was resolved, which is a fact worth
                    // showing rather than a blank cell.
                    <span className="text-muted-foreground">no localized name</span>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </section>
  );
}

/**
 * The transport failed. Phase 10 replaces this with the real route-level error boundary; what it
 * must keep is presenting the correlation identifier and nothing else — no internal chain, no
 * host detail.
 */
function TransportFailure({ error }: { error: Error }) {
  // Three distinct failures, and they must not share a sentence. `HostRejectedError` extends
  // `DocumentationTransportError`, so testing the base class first would tell someone the host was
  // unreachable while showing them a correlation identifier the host itself minted — a diagnosis
  // contradicted by the evidence printed next to it. Most specific first.
  if (error instanceof HostRejectedError) {
    return (
      <Alert>
        <AlertTitle>The documentation could not be read</AlertTitle>
        <AlertDescription>
          The documentation host reached this request and could not complete it. Quote this
          identifier when reporting it: <code className="font-mono">{error.correlationId}</code>.
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <Alert>
      <AlertTitle>The documentation could not be read</AlertTitle>
      <AlertDescription>
        {error instanceof DocumentationTransportError
          ? "The request did not reach the documentation host, or the host answered in a shape this build does not understand."
          : "Something went wrong while loading this page."}
      </AlertDescription>
    </Alert>
  );
}
