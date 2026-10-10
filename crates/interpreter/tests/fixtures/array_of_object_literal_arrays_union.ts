// Arrays of object literals with differing fields join as a union of array
// types, as in tsc, and an empty array joins any of them.
function main(): void {
  const a = [[{ p: 1 }], [{ q: 2 }]];
  const b = [[], [{ p: 1 }], [{ q: 2 }]];
  assert(JSON.stringify(a) === '[[{"p":1}],[{"q":2}]]', "arrays of differing objects join");
  assert(JSON.stringify(b) === '[[],[{"p":1}],[{"q":2}]]', "an empty array joins them");
  let found = 0;
  for (const inner of b) {
    for (const item of inner) {
      if ("p" in item) found += item.p;
      if ("q" in item) found += item.q * 10;
    }
  }
  assert(found === 21, "each array keeps its own element type");
}
