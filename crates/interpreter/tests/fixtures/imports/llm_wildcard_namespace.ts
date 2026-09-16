import * as llm from "submilli:llm";

function main(): void {
  // `* as ns` is a synonym for the default namespace import, so it binds the
  // same package surface and reaches the same gate.
  const c = llm.call("claude-haiku-4-5", "Summarize this.");
  assert(c.ok, "* as ns synonym binds the package as a namespace");
  assert(llm.batch("claude-haiku-4-5", ["a"]).length === 1, "and reaches batch too");
}
