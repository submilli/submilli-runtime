// R6: a response that does not conform to `T` throws a catchable `TypeError`
// rather than arriving statically typed and unverified. The schema sent to the
// provider is advisory — a model can ignore it, answer in the wrong shape, or
// be served by a provider that drops it — so the structural check is what
// actually holds the guarantee. Without the throw, a program would read
// `s.level` as a string that is really a number and carry the corruption
// forward silently.
//
// R13 binds the failure too, and this is the sharper half: a type error must
// not become a disclosure channel. The message names the expected type and the
// runtime kind it found, and never echoes the prompt or the completion — both
// of which a model call carries in quantity.
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // The provider answers with `level` as a number where the interface says
  // string, so the check must reject it.
  let threw = false;
  let message = "";
  try {
    llm.call<Severity>("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
    assert(false, "a non-conforming response must not pass the check");
  } catch (e: TypeError) {
    threw = true;
    message = e.message;
  }

  // It throws, and as a `TypeError` specifically — a class a program can
  // branch on to retry, fall back, or use the untyped form instead.
  assert(threw, "a non-conforming response throws");
  assert(message.length > 0, "the failure carries a message");
  assert(message.indexOf("type mismatch") >= 0, "the failure identifies itself as a type mismatch");

  // R13: the prompt never reaches the message.
  assert(message.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "the message never echoes the prompt");

  // Nor does the offending completion — the value that failed the check is
  // model output, which is exactly what must not leak into an error string.
  assert(message.indexOf("malformed on purpose") < 0, "the message never echoes the completion");

  // The same rejection reaches `batch<T>`: one non-conforming element throws
  // for the batch rather than yielding a wrongly-typed element.
  let batchThrew = false;
  let batchMessage = "";
  try {
    llm.batch<Severity[]>("claude-haiku-4-5", ["CONFIDENTIAL-PROMPT-TEXT"]);
    assert(false, "a non-conforming batch element must not pass the check");
  } catch (e: TypeError) {
    batchThrew = true;
    batchMessage = e.message;
  }
  assert(batchThrew, "a typed batch throws on a non-conforming element");
  assert(batchMessage.indexOf("CONFIDENTIAL-PROMPT-TEXT") < 0, "and never echoes the prompt either");
  assert(batchMessage.indexOf("malformed on purpose") < 0, "nor the completion");

  // The untyped form is the documented alternative when responses may not
  // conform: it returns the envelope instead of throwing, so the caller can
  // inspect the raw text and decide.
  const raw = llm.call("claude-haiku-4-5", "CONFIDENTIAL-PROMPT-TEXT");
  assert(raw.ok, "the untyped form does not apply a structural check");
  assert(raw.text !== undefined, "and hands back the raw text for the caller to judge");

  // Execution continues past a caught mismatch — the check refuses a value, it
  // does not end the program.
  assert(true, "a caught type mismatch leaves the execution usable");
}
