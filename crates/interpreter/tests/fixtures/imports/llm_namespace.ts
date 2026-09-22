import llm from "submilli:llm";

function main(): void {
  const c = llm.call("claude-haiku-4-5", "Summarize this.");
  assert(c.ok, "namespace import dispatches a model call");
  assert(llm.models().length > 0, "and reaches discovery through the same binding");
}
