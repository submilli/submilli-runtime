import { render, screen } from "@testing-library/react";
import { createRef } from "react";
import { expect, test } from "vitest";

import { ChatPane } from "./ChatPane";

const pane = (text: string, role: "agent" | "user") =>
  render(
    <ChatPane
      items={[{ kind: "message", who: role === "agent" ? "Agent" : "You", role, text }]}
      empty=""
      box={{ placeholder: "Reply in this session…", disabled: false, primary: true }}
      text=""
      scrollRef={createRef()}
      onScroll={() => {}}
      onText={() => {}}
      onSend={() => {}}
      onStop={() => {}}
      onNew={() => {}}
      onPick={() => {}}
    />,
  );

test("the agent's Markdown is drawn formatted, and nothing in it becomes markup or a link", () => {
  const { container } = pane("Total: **$61.50**\n\n- one\n- two\n\n<img src=x onerror=alert(1)> [click](javascript:alert(1)) <script>alert(1)</script>", "agent");
  expect(screen.getByText("$61.50").tagName).toBe("STRONG");
  expect(container.querySelectorAll("li")).toHaveLength(2);
  expect(container.querySelector("img, script, a")).toBeNull();
  expect(container.textContent).toContain("<img src=x onerror=alert(1)> click (javascript:alert(1)) <script>alert(1)</script>");
});

test("what you typed shows as you typed it, stars and all", () => {
  const { container } = pane("is **this** bold?", "user");
  expect(container.querySelector("strong")).toBeNull();
  expect(container.textContent).toContain("is **this** bold?");
});
