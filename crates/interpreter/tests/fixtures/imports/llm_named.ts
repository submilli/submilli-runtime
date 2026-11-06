import { text, texts, models } from "submilli:llm";

function main(): void {
  assert(text("claude-haiku-4-5", "Summarize this.").ok, "named import dispatches a call");
  assert(texts("claude-haiku-4-5", ["a", "b"]).length === 2, "named import dispatches a batch");
  assert(models().length > 0, "named import reaches discovery");
}
