import { call, batch, models } from "submilli:llm";

function main(): void {
  assert(call("claude-haiku-4-5", "Summarize this.").ok, "named import dispatches a call");
  assert(batch("claude-haiku-4-5", ["a", "b"]).length === 2, "named import dispatches a batch");
  assert(models().length > 0, "named import reaches discovery");
}
