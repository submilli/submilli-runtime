import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";

import { fixture } from "../model/testdata";
import { initialSetup, type SetupForm } from "../model/setup";
import type { Status } from "../api/types";
import { Setup } from "./Setup";

afterEach(cleanup);

function renderSetup(status = fixture<Status>("status.json")) {
  const Wrapper = () => {
    const [form, setForm] = useState<SetupForm>(() => initialSetup(status));
    return <Setup status={status} form={form} onForm={setForm} onStart={vi.fn()} onBlueprint={vi.fn()} onPackages={vi.fn()} keys={{ save: async () => ({ ok: true, result: { variable: "", from: null }, text: "" }), done: vi.fn() }} />;
  };
  return render(<Wrapper />);
}

test("variable Done focuses the first missing required application variable", async () => {
  const user = userEvent.setup();
  renderSetup();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const variable = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(variable);
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));

  expect(variable).toHaveAttribute("aria-invalid", "true");
  expect(variable).toHaveFocus();
});

test("JSON mode marks missing required variables inline and focuses the editor", async () => {
  const user = userEvent.setup();
  renderSetup();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const variable = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(variable);
  await user.click(screen.getByRole("button", { name: "Edit as JSON" }));
  const editor = screen.getByRole("textbox", { name: "Session variables as JSON" });
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));

  expect(editor).toHaveAttribute("aria-invalid", "true");
  expect(editor).toHaveFocus();
  expect(document.getElementById("json-help")).toHaveTextContent("Still needed: customerId.");
});

test("model Done opens collapsed Advanced model options before focusing invalid max output", async () => {
  const user = userEvent.setup();
  renderSetup();
  const advanced = screen.getByText(/Advanced model options/);
  await user.click(advanced);
  const max = screen.getByRole("textbox", { name: /Max output/ });
  await user.type(max, "-1");
  await user.click(advanced);
  expect(max).not.toBeVisible();

  await user.click(screen.getByRole("button", { name: "Done configuring model" }));

  expect(max).toBeVisible();
  expect(max).toHaveFocus();
  expect(max).toHaveAttribute("aria-invalid", "true");
});

test("model Done opens nested provider settings before focusing invalid provider JSON", async () => {
  const user = userEvent.setup();
  renderSetup();
  await user.click(screen.getByText(/Advanced model options/));
  const providerSummary = screen.getByText(/Provider settings/);
  await user.click(providerSummary);
  const providerJson = screen.getByRole("textbox", { name: "Provider settings as JSON" });
  fireEvent.change(providerJson, { target: { value: '{"model":"override"}' } });
  await user.click(providerSummary);
  await user.click(screen.getByText(/Advanced model options/));
  expect(providerJson).not.toBeVisible();

  await user.click(screen.getByRole("button", { name: "Done configuring model" }));

  expect(providerJson).toBeVisible();
  expect(providerJson).toHaveFocus();
  expect(providerJson).toHaveAttribute("aria-invalid", "true");
});


test("two steps validate, summarize draft values and reopen without starting a session", async () => {
  const user = userEvent.setup();
  renderSetup();
  expect(screen.queryByRole("textbox", { name: /customerId/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Bind application variables" })).toBeDisabled();
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  expect(screen.getByRole("button", { name: /Configure model.*Done.*Edit/ })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByText("Required by blueprint billing to start this session.")).toBeVisible();
  const variable = screen.getByRole("textbox", { name: /customerId/ });
  await user.clear(variable);
  await user.type(variable, "cus_draft");
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  expect(screen.getByRole("button", { name: /Bind application variables.*Ready.*Edit.*customerId: cus_draft/ })).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByRole("button", { name: "Start session" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: /Bind application variables.*Ready/ }));
  expect(screen.getByRole("textbox", { name: /customerId/ })).toHaveValue("cus_draft");
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
});


test("a required variable named model stays in variable validation and blocks readiness", async () => {
  const user = userEvent.setup();
  const status = fixture<Status>("status.json");
  status.declared_variables = [{ name: "model", required: true }];
  status.binding.variables = {};
  renderSetup(status);
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const variable = screen.getByRole("textbox", { name: /model.*required/ });
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  expect(variable).toHaveAttribute("aria-invalid", "true");
  expect(variable).toHaveFocus();
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
  await user.type(variable, "application_value");
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  expect(screen.getByRole("button", { name: "Start session" })).toBeVisible();
});

test("blueprint selection uses its packages and required draft values, then resets completion", async () => {
  const user = userEvent.setup();
  const status = fixture<Status>("status.json");
  status.blueprints = [
    { ...status.blueprint, packages: status.packages, declared_variables: status.declared_variables, blueprint_status: status.blueprint_status },
    { name: "support", path: "/demo/support.yaml", packages: [{ name: "@acme/support", version: "1", origin: "blueprint", importable: true, project: true }], declared_variables: [{ name: "ticketId", required: true, example: "ticket_123" }], blueprint_status: { version: 9, refused: null } },
  ];
  renderSetup(status);
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "support");
  expect(screen.getByText("Packages: @acme/support")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Done configuring model" }));
  const ticket = screen.getByRole("textbox", { name: /ticketId/ });
  expect(screen.queryByRole("textbox", { name: /customerId/ })).not.toBeInTheDocument();
  expect(screen.getByText("Required by blueprint support to start this session.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  expect(ticket).toHaveAttribute("aria-invalid", "true");
  await user.click(screen.getByRole("button", { name: "Use example" }));
  expect(ticket).toHaveValue("ticket_123");
  await user.click(screen.getByRole("button", { name: "Done preparing variables" }));
  expect(screen.getByRole("button", { name: "Start session" })).toBeVisible();
  await user.selectOptions(screen.getByRole("combobox", { name: "Blueprint" }), "billing");
  expect(screen.getByRole("button", { name: "Configure model" })).toHaveAttribute("aria-expanded", "true");
  expect(screen.queryByRole("button", { name: "Start session" })).not.toBeInTheDocument();
});
