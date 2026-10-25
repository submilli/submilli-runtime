import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test, vi } from "vitest";

import { packageDocsKey, type PackageDocs, type Status } from "../api/types";
import { PackagesDialog, type PackageLine } from "./Dialogs";

const status = {
  project: "billing",
  blueprint: { name: "billing", path: "blueprint.subm" },
  url: "http://localhost",
  chat_models: [],
  packages: [
    { name: "@acme/billing", version: "0.1.0", origin: "project", importable: true, project: true },
    { name: "@acme/audit", version: "1.2.0", origin: "installed", importable: false, project: false },
  ],
  running: false,
  stopping: false,
  blueprint_status: { version: 2, refused: null },
  binding: { variables: {}, secrets: [] },
  declared_variables: [],
} satisfies Status;

const packages: PackageLine[] = [
  { name: "@acme/billing", rules: { line: 12, count: 1 } },
  { name: "@acme/audit", rules: { line: 20, count: 2 } },
];

const docs: Record<string, PackageDocs> = {
  [packageDocsKey("billing", "@acme/billing")]: {
    package: "@acme/billing",
    bytes: 30,
    truncated: false,
    note: null,
    untrusted: { markdown: "# Billing\n\nUse `listCharges` for customer charges." },
  },
  [packageDocsKey("billing", "@acme/audit")]: {
    package: "@acme/audit",
    bytes: 28,
    truncated: false,
    note: null,
    untrusted: { markdown: "# Audit\n\nUse `recordEvent` for audit entries." },
  },
};

test("the package dialog keeps multiple package guides selectable and one at a time", async () => {
  const user = userEvent.setup();
  const onOpenRules = vi.fn();
  render(
    <PackagesDialog
      blueprint="billing"
      inForceLabel="v2"
      status={status}
      packages={packages}
      docs={docs}
      docsErrors={{}}
      onLoadDocs={vi.fn()}
      onClose={vi.fn()}
      onOpenRules={onOpenRules}
    />,
  );

  const dialog = screen.getByRole("dialog", { name: "Packages in billing v2" });
  expect(within(dialog).getByRole("button", { name: "@acme/billing" })).toHaveAttribute("aria-pressed", "true");
  expect(within(dialog).getByRole("button", { name: "@acme/audit" })).toHaveAttribute("aria-pressed", "false");
  expect(within(dialog).getByText("Billing", { selector: ".pg-md-head" })).toBeInTheDocument();
  expect(within(dialog).queryByText("Audit", { selector: ".pg-md-head" })).not.toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "View its rule" })).toBeInTheDocument();
  expect(within(dialog).getByRole("heading", { name: "API guide" })).toBeInTheDocument();
  expect(within(dialog).getByRole("heading", { name: "Operation rules" })).toBeInTheDocument();
  expect(within(dialog).getByRole("heading", { name: "Internal permissions" })).toBeInTheDocument();
  expect(within(dialog).getByText(/Separate public operation rules are not recorded/)).toBeInTheDocument();

  await user.click(within(dialog).getByRole("button", { name: "@acme/audit" }));
  expect(within(dialog).getByText("Audit", { selector: ".pg-md-head" })).toBeInTheDocument();
  expect(within(dialog).queryByText("Billing", { selector: ".pg-md-head" })).not.toBeInTheDocument();
  expect(within(dialog).getByText("recordEvent")).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: "View its 2 rules" })).toBeInTheDocument();
  expect(within(dialog).getByText(/The package has 2 explicit permission rules/)).toBeInTheDocument();

  await user.click(within(dialog).getByRole("button", { name: "View its 2 rules" }));
  expect(onOpenRules).toHaveBeenCalledWith("@acme/audit", 20);
});
