// The `schema` slot is compiler-filled, and writing it by hand is a compile
// error rather than a call that quietly does something else.
//
// This is a soundness boundary, not a style rule. The host decides a call is
// typed by looking at whether the schema argument is non-null, while the
// structural check that makes a typed call safe is a cast the typechecker emits
// only when a type argument was written. A hand-written schema sets the first
// without the second, so raw parsed JSON would reach the guest wearing the
// `Completion` interface type: reading `ok` off a JSON object returns a
// fabricated `true`, and reading `text` traps outside the error taxonomy
// entirely. It would also hand guest-written bytes to the provider as the
// schema for the request.
//
// Both entry points are refused, and so is the typed form — the argument is
// meaningless there too, since the compiler overwrites the slot.
// expect-error: the schema argument is filled by the compiler
// expect-error: the schema argument is filled by the compiler
// expect-error: the schema argument is filled by the compiler
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

  assert(handWritten.ok, "unreachable — the call above does not compile");
  assert(batched.length === 1, "unreachable");
  assert(typed.level === "", "unreachable");
}
