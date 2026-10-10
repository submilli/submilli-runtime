// The schema is advisory; the check is not. This is the distinction the typed
// form rests on, and it is not hypothetical: of the providers this runtime
// targets, some honor a response schema, some accept it and drift from it, and
// at least one silently degrades a referenced subschema to "any". A typed call
// that trusted the schema alone would hand back whatever arrived, wearing a
// type it never verified.
//
// Here the provider ignores the schema completely and answers in prose. The
// typed path is the only thing standing between that prose and a value the
// program would read as a `Severity`, so it must throw.
//
// Prose fails *earlier* than a wrong-shaped object does: it is not JSON at all,
// so it is refused at the parse step and arrives as a `SyntaxError` rather than
// the `TypeError` a well-formed mismatch produces (`llm_typed_mismatch`). The
// two are deliberately different classes — "the provider did not answer in the
// requested format" and "the provider answered in the format but the wrong
// shape" are different problems with different fixes — and both are catchable.
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // Prose where an object was requested fails before any structural check.
  let threw = false;
  let message = "";
  try {
    llm.call<Severity>("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
    assert(false, "prose must not pass as a typed result");
  } catch (e: SyntaxError) {
    threw = true;
    message = e.message;
  }

  assert(threw, "a provider that ignored the schema still fails the typed call");
  assert(message.indexOf("not JSON") >= 0, "the failure names the format the response missed");

  // It names the operation, so a program calling several ops can attribute it.
  assert(message.indexOf("llm.call") >= 0, "the failure names the operation that produced it");

  // R13: neither the prompt nor the model's prose reaches the error.
  assert(message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the message never echoes the prompt");
  assert(message.indexOf("honestly") < 0, "the message never echoes the completion");

  // The untyped form accepts the same response without complaint — it promises
  // no shape, so there is nothing to violate. This is the documented fallback
  // when a provider cannot be relied on to honor a schema.
  const raw = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(raw.ok, "the untyped form accepts a response of any shape");
  assert(raw.text !== undefined, "and hands the prose back for the caller to parse");
  assert((raw.text as string).length > 0, "with the text intact");
}
