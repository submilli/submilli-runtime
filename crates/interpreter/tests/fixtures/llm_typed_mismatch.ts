// R6: a response that does not conform to `T` is an `invalid-output` failure
// entry rather than a value that arrives statically typed and unverified. The
// schema sent to the provider is advisory — a model can ignore it, answer in
// the wrong shape, or be served by a provider that drops it — so the
// structural check is what actually holds the guarantee. Without it, a program
// would read `level` as a string that is really a number and carry the
// corruption forward silently.
//
// R13 binds the failure too, and this is the sharper half: a type error must
// not become a disclosure channel. The message names the expected type, and
// never echoes the prompt or the completion — both of which a model call
// carries in quantity.
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // The provider answers with `level` as a number where the interface says
  // string, so the check must reject it.
  const s = llm.call<Severity>("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(!s.ok, "a non-conforming response is not a clean completion");
  if (s.ok) return;

  // It is reported as invalid output — the reason a program branches on to
  // retry, fall back, or use the untyped form instead.
  assert(s.reason === "invalid-output", "a non-conforming response is invalid output");
  assert(s.message.indexOf("does not match `Severity`") >= 0, "the message names the expected type");

  // R13: the prompt never reaches the message.
  assert(s.message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the message never echoes the prompt");

  // Nor does the offending completion — the value that failed the check is
  // model output, which is exactly what must not leak into an error string.
  assert(s.message.indexOf("malformed on purpose") < 0, "the message never echoes the completion");

  // A typed failure carries no partial text: `value` could never have held it.
  assert(s.text === undefined, "a typed mismatch keeps the completion off the envelope");

  // The same rejection reaches `batch<T>`: a non-conforming element is its
  // own failure entry, and its siblings are unaffected.
  const many = llm.batch<Severity>("claude-haiku-4-5", ["CONFIDENTIAL-PROMPT-TEXT"]);
  assert(many.length === 1, "a batch with a non-conforming element still returns it");
  const element = many[0];
  assert(!element.ok, "a typed batch marks a non-conforming element");
  if (element.ok) return;
  assert(element.reason === "invalid-output", "with the same reason");
  assert(element.message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "and never echoes the prompt either");
  assert(element.message.indexOf("malformed on purpose") < 0, "nor the completion");

  // The untyped form is the documented alternative when responses may not
  // conform: it promises no shape, so the raw text comes back for the caller
  // to inspect and decide.
  const raw = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(raw.ok && raw.value.length > 0, "the untyped form hands back the raw text for the caller to judge");
}
