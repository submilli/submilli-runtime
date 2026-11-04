// `call<T>` takes a model and a prompt, `batch<T>` a model and prompts:
// nothing else. The JSON Schema a typed call sends and the check it runs
// travel in the type descriptor of `T` — part of how a generic function is
// called, built by the compiler — so a program that writes a schema or a
// check as an argument hits the ordinary arity error: it is not surface, and
// there is no slot for it to land in.
//
// This is a soundness boundary, not a style rule. The host decides a value is
// a `T` by the descriptor the compiler built and trusts its predicate to be
// the structural test of `T`; a hand-written schema or check would let raw
// parsed JSON reach the guest wearing the declared type. The text forms take
// a program's own schema instead, and never treat the answer as typed.
// expect-error: `llm.call` expects 2 argument(s), got 3
// expect-error: `llm.batch` expects 2 argument(s), got 3
// expect-error: `llm.call` expects 2 argument(s), got 4
// expect-error: expected 2-3 argument(s), got 4
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // Typed with a hand-written schema: the compiler emits its own.
  const typed = llm.call<Severity>("m", "classify this", "{}");

  // The same through `batch`.
  const batched = llm.batch<Severity>("m", ["classify this"], "{}");

  // A hand-written check is refused for the same reason.
  const checked = llm.call<Severity>("m", "classify this", "{}", (v: unknown): unknown => v);

  // The text form takes a schema, and nothing beyond it.
  const text = llm.text("m", "classify this", "{}", (v: unknown): unknown => v);

  assert(typed.ok, "unreachable — the calls above do not compile");
  assert(batched.length === 1, "unreachable");
  assert(checked.ok, "unreachable");
  assert(text.ok, "unreachable");
}
