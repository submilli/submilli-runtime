// An object literal assigned to a union of object types may name any field
// some member declares, as in tsc. Only a field typed as a unit type or a
// union of them (a tag) rules members out, and a member without the tag field
// stays a candidate.

type AB = { a: number } | { b: string };
type Tagged = { k: "x"; a: number } | { b: number };
type NotATag = { a: number; b: string } | { a: string; c: number };
type NullableNotATag = { a: string | null; b: number } | { a: number; c: number };
type NamedIndex = { a: number } | { b: number; [key: string]: number };
type WithEmpty = { a: number } | {};
interface Counter {
  count: number;
  next(): number;
}
type CounterOrLabel = Counter | { label: string };

function describe(x: AB): string {
  if ("a" in x) {
    return `a=${x.a}`;
  }
  return `b=${x.b}`;
}

function main(): void {
  const both: AB = { a: 1, b: "x" };
  assert(describe(both) === "a=1", "a field from each member");
  const untagged: Tagged = { k: "x", a: 1, b: 2 };
  assert("k" in untagged, "a member without the tag field stays");
  const loose: NotATag = { a: 1, b: "s", c: 2 };
  assert("c" in loose, "a field typed `number | string` isn't a tag");
  const nullable: NullableNotATag = { a: "s", b: 1, c: 2 };
  assert("c" in nullable, "a field typed `string | null` isn't a tag");
  const indexed: NamedIndex = { a: 1, z: 2 };
  assert("z" in indexed, "an index signature takes any field");
  const open: WithEmpty = { a: 1, z: 2 };
  assert("a" in open, "an empty member takes any field");
  const counter: CounterOrLabel = { count: 1, next: () => 2 };
  assert("next" in counter, "an interface's methods are its fields");
  console.log("ok");
}
