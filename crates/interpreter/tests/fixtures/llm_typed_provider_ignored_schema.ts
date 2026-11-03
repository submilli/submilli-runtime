// The schema is advisory; the check is not. This is the distinction the typed
// form rests on, and it is not hypothetical: of the providers this runtime
// targets, some honor a response schema, some accept it and drift from it, and
// at least one silently degrades a referenced subschema to "any". A typed call
// that trusted the schema alone would hand back whatever arrived, wearing a
// type it never verified.
//
// Here the provider ignores the schema completely and answers in prose. The
// typed path is the only thing standing between that prose and a value the
// program would read as a `Severity`, so it must refuse.
//
// Prose fails *earlier* than a wrong-shaped object does: it is not JSON at all,
// so it is refused at the parse step. Both land on the same `invalid-output`
// reason — "the provider did not answer in the requested format" and "the
// provider answered in the format but the wrong shape" are both the model's
// output being unusable — and the message tells them apart.
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // Prose where an object was requested fails before any structural check.
  const s = llm.call<Severity>("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(!s.ok, "a provider that ignored the schema still fails the typed call");
  if (s.ok) return;
  assert(s.reason === "invalid-output", "prose where JSON was asked for is invalid output");
  assert(s.message.indexOf("not JSON") >= 0, "the failure names the format the response missed");

  // R13: neither the prompt nor the model's prose reaches the message.
  assert(s.message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the message never echoes the prompt");
  assert(s.message.indexOf("honestly") < 0, "the message never echoes the completion");

  // The untyped form accepts the same response without complaint — it promises
  // no shape, so there is nothing to violate. This is the documented fallback
  // when a provider cannot be relied on to honor a schema.
  const raw = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(raw.ok && raw.value.length > 0, "the untyped form accepts a response of any shape, text intact");
}
