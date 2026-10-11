// Object rest copies the source's other fields into a new object, as in
// JavaScript: writing through the rest leaves the source alone, and the fields
// destructured beside it are not in it. A source that is a union with a
// dictionary member reads a named field from either member and copies whatever
// other fields the value holds.
interface Dict {
  [key: string]: number;
}

function split(x: Dict | { a: string; b: string }): string {
  const { b, ...others } = x;
  return `${JSON.stringify(b)} ${JSON.stringify(others)}`;
}

function readB(x: Dict | { a: string; b: string }): string {
  return String(x.b ?? "absent");
}

function pick(flag: boolean): { a: number; b: number } | { a: number; c: string } {
  return flag ? { a: 1, b: 2 } : { a: 3, c: "x" };
}

function main(): void {
  const source = { a: 1, b: 2, c: 3 };
  const { b, ...rest } = source;
  rest.a = 5;
  assert(source.a === 1, "the rest is a copy");
  assert(b === 2, "the destructured field");
  assert(JSON.stringify(rest) === '{"a":5,"c":3}', "the rest omits the destructured field");

  const { ...whole } = source;
  whole.c = 9;
  assert(source.c === 3 && whole.c === 9, "a rest with nothing beside it copies everything");

  assert(split({ a: "s", b: "t" }) === '"t" {"a":"s"}', "the object member");
  assert(split({ b: 2, z: 3 }) === '2 {"z":3}', "the dictionary member");
  assert(readB({ b: 4 }) === "4", "a dictionary backs a named read");
  assert(readB({ z: 4 }) === "absent", "an absent dictionary key");

  const { a, ...tail } = pick(false);
  assert(a === 3, "a union's shared field");
  assert(JSON.stringify(tail) === '{"c":"x"}', "a union's other fields");
}
