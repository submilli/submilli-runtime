// expect-error: `Number` is a namespace, not a value — `?.` has nothing to guard
// expect-error: write `Number.EPSILON`, not `Number?.EPSILON`
// expect-error: `console` is a namespace, not a value — `?.` has nothing to guard
// expect-error: `Temporal.Instant` is a namespace, not a value — `?.` has nothing to guard
// expect-error: write `Temporal.Instant.from`, not `Temporal.Instant?.from`
// expect-error: `Temporal.Duration` is a namespace, not a value — `?.` has nothing to guard
// expect-error: `String` is a namespace, not a value — `?.` has nothing to guard
// expect-error: write `String(…)`, not `String?.(…)`
// expect-error: this expression is a namespace, not a value — `?.` has nothing to guard
// expect-error: drop the `?` — a namespace is never null
// A static-dispatch interface binding lowers to an inert typed null that every
// call site drops, so a `?.` on it would see that null, short-circuit, and yield
// `null` where the member was asked for.

function main(): void {
  const e = Number?.EPSILON;
  console?.log("x");
  // Reached by a dotted path.
  const i = Temporal.Instant?.from("2020-01-01T00:00:00Z");
  // Parenthesized: the parens are not part of the name.
  const d = (Temporal.Duration)?.from("PT1S");
  // A call, not a property access: the `.` goes with the `?`.
  const s = String?.(42);
  // An index step has no legal rewrite, so the fix names none.
  const x = Number?.[0];
  // The base spells no name at all, so the message quotes none.
  const cond = true;
  const t = (cond ? Number : Number)?.EPSILON;
}
