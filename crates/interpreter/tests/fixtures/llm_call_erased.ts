// R7: `call<T>` under a *user* generic is rejected at compile time. A type
// parameter of the calling function has no runtime representation — it is
// erased before the program runs — so there is nothing for the structural check
// to test the response against, and nothing concrete to emit a JSON Schema
// from. The call would compile to a check that cannot be written.
//
// This is the same erasure gate that screens `as` targets, and it names erasure
// as the reason rather than reporting a vague unsupported-type error, because
// the fix is structural: the caller has to name a concrete shape at the call
// site instead of forwarding its own parameter.
// expect-error: generic type parameters are erased at runtime
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

// `T` here is erased, so the typed call inside cannot be checked.
function classify<T>(prompt: string): T {
  return llm.call<T>("claude-haiku-4-5", prompt);
}

function main(): void {
  const s = classify<Severity>("Classify this.");

  assert(s.level === "", "unreachable — the call above does not compile");
}
