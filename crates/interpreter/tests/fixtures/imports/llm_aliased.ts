import { call as ask, models as catalog } from "submilli:llm";

function main(): void {
  // The gate and the typed lowering key on the resolved export, never on
  // whatever the caller happened to name it.
  assert(ask("claude-haiku-4-5", "Summarize this.").ok, "aliased import dispatches a call");
  assert(catalog().length > 0, "aliased import reaches discovery");
}
