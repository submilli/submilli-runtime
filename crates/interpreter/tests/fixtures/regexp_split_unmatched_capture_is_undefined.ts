// A `RegExp` separator inserts each match's captures between the parts, and a
// group that didn't participate is `undefined`, as in JavaScript.
function main(): void {
  const parts = "a1b".split(/(x)?1/);
  assert(parts.length === 3, "the capture slot is inserted");
  assert(parts[0] === "a" && parts[2] === "b", "the parts around it");
  assert((parts[1] as unknown) === undefined, "an unmatched group is undefined");
}
