// An object literal expected as a tagged union with `null` takes the member its
// tag names, as in tsc, even when the members share a field of different types.
type A = { k: "a"; x: number };
type B = { k: "b"; x: string };

function direct(n: number): A | B | null {
  return { k: "a", x: n };
}

function chosen(n: number): A | B | null {
  return n > 0 ? { k: "a", x: n } : n < 0 ? { k: "b", x: "neg" } : null;
}

function main(): void {
  assert(JSON.stringify(direct(1)) === '{"k":"a","x":1}', "the tag picks A");
  assert(JSON.stringify(chosen(2)) === '{"k":"a","x":2}', "a branch picks A");
  assert(JSON.stringify(chosen(-1)) === '{"k":"b","x":"neg"}', "a branch picks B");
  assert(chosen(0) === null, "the null branch");
  const v = chosen(3);
  if (v !== null && v.k === "a") {
    assert(v.x + 1 === 4, "the A member's field is a number");
  }
  const held: A | B | null = { k: "b", x: "s" };
  assert(held !== null && held.k === "b", "a declaration picks B");
}
