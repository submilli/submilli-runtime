class Joiner {
  concat(first: number[] = []): number[] { first.push(1); return first; }
}
let value: number[] | Joiner = [1];
function change(): boolean { value = new Joiner(); return false; }
// The declared return type lets both front ends re-check the post-call value.
function readCurrent(): number[] | Joiner { return value; }
function main(): void {
  if (!Array.isArray(value) || change()) return;
  const actual = readCurrent();
  if (Array.isArray(actual)) throw new Error("change must leave a Joiner");
  assert(actual.concat().length === 1);
  assert(actual?.concat().length === 1);
}
