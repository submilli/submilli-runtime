// A function literal in an object literal checked against a union of object
// types takes its parameter types from the union of that field's types, as
// in tsc, when no single member of the union is chosen for the literal.
type Lengths = { measure: (text: string, extra: number) => number };
type Labels = { measure: (text: string, extra: number) => string; note?: string };
interface Counter {
  count: (items: string[]) => number;
}
interface Joiner {
  count: (items: string[]) => string;
  separator?: string;
}

function apply(shape: Lengths | Labels): string {
  return `${shape.measure("abc", 2)}`;
}

function main(): void {
  const lengths: Lengths | Labels = { measure: (text, extra) => text.length + extra };
  const counter: Counter | Joiner = { count: (items) => items.length };
  assert(apply(lengths) === "5");
  assert(apply({ measure: (text, extra) => text.repeat(extra) }) === "abcabc");
  assert(`${counter.count(["a", "b"])}` === "2");
}
