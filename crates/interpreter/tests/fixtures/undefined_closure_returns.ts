// An unannotated closure that returns a value on some paths and nothing on
// others returns `undefined` there, so its return type is `T | undefined`, as
// TypeScript infers. A bare `return;`, `return undefined;` and falling off the
// end all mean the same. A `() => undefined` context accepts a body with no
// `return`, as TypeScript 5.1 does.
function one(): number { return 1; }
class Holder { value: number | undefined; label?: string; }
function main(): void {
  const key: string = ["a"][0];
  const bare = () => { if (key === "a") { return; } return one(); };
  const explicit = function () { if (key === "b") { return 1; } return undefined; };
  const fallsOff = (x: number) => { if (x > 0) { return x; } };
  const nullable = () => { if (key === "a") { return; } return null; };
  const done: () => undefined = () => {};
  assert(bare() === undefined, "a bare return yields undefined");
  assert(explicit() === undefined, "an explicit undefined return");
  assert(fallsOff(2) === 2 && fallsOff(-1) === undefined, "falling off the end");
  assert(nullable() === undefined, "null and undefined returns join");
  assert(done() === undefined, "a body with no return under an undefined context");
  const typed: number | undefined = fallsOff(3);
  assert(typed === 3, "the inferred type is number | undefined");
  const mapped = [1, -1].map((x) => { if (x > 0) { return x; } });
  assert(mapped[0] === 1 && mapped[1] === undefined, "a callback that sometimes returns");
  const h = new Holder();
  assert(h.value === undefined && h.label === undefined, "a field admitting undefined starts empty");
}
