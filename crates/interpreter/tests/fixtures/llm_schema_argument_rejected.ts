// `call` takes a model and a prompt, `batch` a model and prompts: nothing
// else. The JSON Schema a typed call sends and the check it runs travel in the
// call's type witness — part of how a generic function is called, built by the
// compiler from `T` — so a program that writes a third argument hits the
// ordinary arity error: it is not surface, and there is no slot for it to land
// in.
//
// This is a soundness boundary, not a style rule. The host decides a call is
// typed by the witness the compiler built, and trusts its predicate to be the
// structural test of `T`; a hand-written schema or check would let raw parsed
// JSON reach the guest wearing the declared type, and would hand
// guest-written bytes to the provider as the schema for the request.
// expect-error: `llm.call` expects 2 argument(s), got 3
// expect-error: `llm.batch` expects 2 argument(s), got 3
// expect-error: `llm.call` expects 2 argument(s), got 3
// expect-error: `llm.call` expects 2 argument(s), got 4
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // Untyped with a hand-written schema.
  const handWritten = llm.call("m", "classify this", "{}");

  // The same through `batch`.
  const batched = llm.batch("m", ["classify this"], "{}");

  // Typed with a hand-written schema: the compiler emits its own, so the
  // argument is meaningless, and it is refused like any extra argument.
  const typed = llm.call<Severity>("m", "classify this", "{}");

  // A hand-written check is refused for the same reason.
  const checked = llm.call("m", "classify this", "{}", (v: unknown): unknown => v);

  assert(handWritten.ok, "unreachable — the call above does not compile");
  assert(batched.length === 1, "unreachable");
  assert(typed.ok, "unreachable");
  assert(checked.ok, "unreachable");
}
