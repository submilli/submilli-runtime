// R7: `call<unknown>` is rejected at compile time. Asking for a *verified* read
// against a type that admits every value is a contradiction — there is no
// runtime check that could fail, and the JSON Schema for `unknown` could only
// be `{}`, which constrains the model to nothing. Accepting it would hand back
// an entirely unvalidated value wearing a validated call's shape, which is
// strictly worse than the untyped form: the untyped form at least tells the
// truth about what it checked.
//
// Two gates would each catch this — the cast gate and the schema gate — and the
// diagnostic points at the two honest alternatives rather than only refusing.
// expect-error: `llm.call<unknown>` would not verify anything
// expect-error: name the shape you expect
import llm from "submilli:llm";

function main(): void {
  // Rejected before any request is made: there is no shape to verify against.
  const v = llm.call<unknown>("claude-haiku-4-5", "Classify this.");

  assert(v !== null, "unreachable — the call above does not compile");
}
