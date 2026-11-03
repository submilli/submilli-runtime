import { classifyTyped } from "@test/triage";

function main(): void {
  // The check closure lives in the package module; the host calls it there.
  assert(classifyTyped("claude-haiku-4-5", "fits") === "spam", "a conforming response is the checked value");
  assert(classifyTyped("claude-haiku-4-5", "prose please") === "invalid-output", "prose is invalid output");
}
