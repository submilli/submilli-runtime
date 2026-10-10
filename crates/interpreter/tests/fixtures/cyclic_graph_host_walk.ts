// The universal vtable walk (`equals`/`hash`/`toJson`) recurses in *host*
// frames, invisible to the engine's own stack limit, so it carries its own
// depth bound: past it the walk raises a catchable RangeError instead of
// exhausting the native stack and aborting the process.
function main(): void {
  const a: unknown[] = [1];
  a[0] = a;
  const b: unknown[] = [1];
  b[0] = b;

  let message = "";
  try {
    const eq = a === b;
    assert(false, "comparing two cyclic arrays should not return");
  } catch (e: RangeError) {
    message = e.message;
  }
  assert(message.includes("nested deeper than"), "the runaway is reported, not fatal");
  assert(message.includes("cycle"), "the message names the usual cause");

  // Mutual recursion (A -> B -> A) reaches the bound the same way.
  const p: unknown[] = [1];
  const q: unknown[] = [2];
  p[0] = q;
  q[0] = p;
  let mutual = "";
  try {
    JSON.stringify(p);
    assert(false, "serializing a mutually recursive graph should not return");
  } catch (e: RangeError) {
    mutual = e.message;
  }
  assert(mutual === message, "the same bound catches serialization");

  // The depth counter unwinds on the throw: ordinary work still succeeds.
  const x: unknown[] = [1, 2];
  const y: unknown[] = [1, 2];
  assert(x === y, "a plain array comparison after a caught runaway");

  // The bound is pinned to `JSON.parse`'s own recursion limit, so a document
  // the runtime accepts round-trips. 100 levels sits inside it.
  let doc = "0";
  for (let i = 0; i < 100; i = i + 1) {
    doc = "[" + doc + "]";
  }
  const parsed = JSON.parse(doc);
  assert(JSON.stringify(parsed) === doc, "a 100-deep document round-trips");

  let nested: unknown[] = [0];
  for (let i = 0; i < 100; i = i + 1) {
    nested = [nested];
  }
  let mirror: unknown[] = [0];
  for (let i = 0; i < 100; i = i + 1) {
    mirror = [mirror];
  }
  assert(nested === mirror, "100 levels of acyclic nesting still compares");
}
