// The `schema` and `check` slots are compiler-filled, and writing either by
// hand is a compile error rather than a call that quietly does something else.
//
// This is a soundness boundary, not a style rule. The host decides a call is
// typed by looking at whether the schema argument is not undefined, and it
// trusts the `check` closure to be the compiler's `value as T`. A hand-written
// schema or check would let raw parsed JSON reach the guest wearing the type
// the call declared, and would hand guest-written bytes to the provider as the
// schema for the request.
//
// Both entry points are refused, and so is the typed form — the argument is
// meaningless there too, since the compiler overwrites the slot.
// expect-error: the schema and check arguments are filled by the compiler
// expect-error: the schema and check arguments are filled by the compiler
// expect-error: the schema and check arguments are filled by the compiler
// expect-error: the schema and check arguments are filled by the compiler
import llm from "submilli:llm";

interface Severity {
  level: string;
  rationale: string;
}

function main(): void {
  // Untyped with a hand-written schema: the case that produced a fabricated
  // `ok` before this was rejected.
  const handWritten = llm.call("m", "classify this", "{}");

  // The same hole through `batch`.
  const batched = llm.batch("m", ["classify this"], "{}");

  // Typed with a hand-written schema. Behaviorally this one was already safe —
  // the compiler overwrites the slot — but accepting an argument that is
  // silently discarded is its own trap, so it is refused too.
  const typed = llm.call<Severity>("m", "classify this", "{}");

  // A hand-written check is refused for the same reason.
  const checked = llm.call("m", "classify this", "{}", (v: unknown): unknown => v);

  assert(handWritten.ok, "unreachable — the call above does not compile");
  assert(batched.length === 1, "unreachable");
  assert(typed.ok, "unreachable");
  assert(checked.ok, "unreachable");
}
