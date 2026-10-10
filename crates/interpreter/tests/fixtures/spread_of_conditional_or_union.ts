// A spread whose source is a conditional or a union of object types copies the
// fields the chosen object has. A field only some alternatives have is optional:
// an absent one leaves an earlier value in place, as in JavaScript. TypeScript
// keeps a spread union as a union, so it rejects reading `copied.y` below; here
// the alternatives merge into one object type, where `y` is optional.
function pick(b: boolean): boolean {
  return b;
}

function main(): void {
  const named: { a: string } = { a: "text" };
  const chosen = { a: 123, ...(pick(true) ? named : {}) };
  assert(chosen.a === "text", "the chosen branch's field is copied");
  const skipped = { a: 123, ...(pick(false) ? named : {}) };
  assert(skipped.a === 123, "a branch without the field keeps the earlier value");

  const missing: { a: string } | null = null;
  const fallback = { a: 1, ...(missing ?? { b: true }) };
  assert(fallback.a === 1, "the fallback object has no `a`");
  assert((fallback.b ?? false) === true, "the fallback's field is copied");

  const either: { x: number } | { x: string; y: boolean } = pick(true)
    ? { x: 1 }
    : { x: "s", y: true };
  const copied = { ...either };
  assert(copied.x === 1, "a field every member has is copied");
  assert((copied.y ?? "absent") === "absent", "a field this member lacks is absent");

  // A field that is present with the value null overwrites; only absence keeps
  // the earlier value.
  const nulled: { k: number } | { k: number; m: string | null } = pick(false)
    ? { k: 1 }
    : { k: 2, m: null };
  const overwritten = { m: "keep" as string | null, ...nulled };
  assert(overwritten.m === null, "a present null is copied");

  // Width subtyping lets an object carry a field its static type doesn't
  // declare, of another type. A field whose value isn't of the declared type
  // reads as absent, rather than as a value of the wrong type.
  const wide = { a: 1, b: 5 };
  const narrow: { a: number } | { a: number; b: string } = wide as { a: number } | { a: number; b: string };
  const spreadWide = { ...narrow };
  assert((spreadWide.b ?? "absent") === "absent", "a field of another type is absent");
}
