import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, test, vi } from "vitest";

import { ProviderKeyStatus } from "./ProviderKeyStatus";

test("shows a saved key with a check and a clear change action", async () => {
  const user = userEvent.setup();
  const onManage = vi.fn();

  render(<ProviderKeyStatus configured onManage={onManage} providerLabel="OpenAI" />);

  expect(screen.getByRole("status")).toHaveTextContent("Key saved");
  expect(document.querySelector(".pg-key-status-icon")).toHaveAttribute("aria-hidden", "true");
  expect(screen.getByRole("button", { name: "Change OpenAI key" })).toHaveTextContent("Change key");
  expect(screen.queryByRole("button", { name: "Add OpenAI key" })).not.toBeInTheDocument();

  await user.click(screen.getByRole("button", { name: "Change OpenAI key" }));
  expect(onManage).toHaveBeenCalledOnce();
});

test("makes a missing key obvious with a primary add action", async () => {
  const user = userEvent.setup();
  const onManage = vi.fn();

  const { container } = render(<ProviderKeyStatus configured={false} onManage={onManage} providerLabel="Gemini" />);
  const view = within(container);

  expect(view.getByRole("status")).toHaveTextContent("No Gemini key saved");
  const add = view.getByRole("button", { name: "Add Gemini key" });
  expect(add).toHaveTextContent("Add key");
  expect(add).toHaveClass("pg-primary");
  expect(view.queryByText("Key saved")).not.toBeInTheDocument();

  await user.click(add);
  expect(onManage).toHaveBeenCalledOnce();
});

test("uses provider key wording when no provider label is supplied", () => {
  const { container } = render(<ProviderKeyStatus configured={false} onManage={vi.fn()} />);
  const view = within(container);

  expect(view.getByRole("status")).toHaveTextContent("No provider key saved");
  expect(view.getByRole("button", { name: "Add provider key" })).toBeInTheDocument();
});
