// An object literal assigned to a union of object types may only name a field
// that some member declares. A field spelled as a literal first rules out the
// members whose same field can't hold it, so `c` below is unknown once
// `a: null` has picked the first member, as in tsc. A union with no object
// member has no fields to check, so only the assignment is reported.
// expect-error: object literal has unknown field `c` for the target type
// expect-error: valid fields: `a`, `b`
// expect-error: object literal has unknown field `b` for the target type
// expect-error: object literal has unknown field `z` for the target type
// expect-error: object literal has unknown field `w` for the target type
// expect-error: remove `w` or rename it to `a`
// expect-error: valid fields: `id`, `name`
// expect-error: expected `number | string`, got `{ a: number }`
// expect-error-count: 9
type AB = { a: number } | { b: string };
type ByNull = { a: null; b: string } | { a: string; c: number };
type Opt = { a?: number } | { b?: string };
interface Named {
  name: string;
}
type NamedOrId = Named | { id: number };

function take(x: AB): void {}

function main(): void {
  const none: AB = { a: 1, c: 4 };
  const pickedByNull: ByNull = { a: null, b: "f", c: 4 };
  const pickedByString: ByNull = { a: "s", b: "f", c: 4 };
  const optional: Opt = { z: 1 };
  const nullable: AB | null = { a: 1, z: 1 };
  take({ a: 1, w: 2 });
  const nested: { p: AB } = { p: { a: 1, z: 1 } };
  const viaInterface: NamedOrId = { name: "n", z: 1 };
  const primitive: string | number = { a: 1 };
}
