// A provider that serves no models returns an empty list rather than throwing.
// The distinction matters because "the operator declared no models" is a
// configuration state a program can reasonably handle — fall back, skip the
// delegation, report that no model was available — whereas a throw would force
// every caller to wrap discovery in a try/catch to learn something benign.
//
// It is deliberately *not* the same as having no provider, which does throw
// (`llm_no_provider`): there the runtime cannot answer the question at all, and
// reporting an empty catalog would misrepresent an unconfigured embedding as a
// deliberately empty one.
import llm from "submilli:llm";

function main(): void {
  // Discovery succeeds and reports nothing.
  const models = llm.models();
  assert(models.length === 0, "a provider serving no models lists as empty");

  // Iterating an empty listing is a no-op rather than an error, so the ordinary
  // selection loop needs no special case.
  let seen = 0;
  for (const m of models) {
    seen = seen + 1;
  }
  assert(seen === 0, "iterating an empty listing visits nothing");

  // Calling anything is still refused — an empty catalog means every model is
  // undeclared, and declaration is authoritative.
  let caught = "";
  try {
    llm.call("claude-haiku-4-5", "Summarize this.");
    assert(false, "no model is callable when the catalog is empty");
  } catch (e: Error) {
    caught = e.message;
  }
  assert(caught.length > 0, "a call against an empty catalog throws catchably");
  assert(caught.indexOf("claude-haiku-4-5") >= 0, "the rejection names the model asked for");

  // With nothing to suggest, the message says so rather than offering an empty
  // list of alternatives.
  assert(caught.indexOf("blueprint") >= 0, "the fix points at declaring a model");
  assert(caught.indexOf("Summarize this.") < 0, "the rejection never echoes the prompt");

  // Discovery is repeatable: an empty catalog is a stable answer, not a
  // one-shot failure that empties a cache.
  assert(llm.models().length === 0, "a second listing reports the same empty catalog");
}
