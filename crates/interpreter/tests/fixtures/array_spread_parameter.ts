// Spreading a parameter into an array literal (SUB-1068). Codegen may widen a
// parameter's value to `unknown`, and the spread must still read it as an array.
function copy(xs: number[]): number[] {
  return [...xs];
}

function wrap(xs: string[]): string[] {
  return ["<", ...xs, ">"];
}

function fromTuple(t: [number, string]): number {
  return [...t].length;
}

function fromReadonly(xs: readonly number[]): number {
  return [...xs].length;
}

function main(): void {
  const source = [3, 1, 2];
  const copied = copy(source);
  source.push(4);
  assert(copied.length === 3, "the copy doesn't share the parameter's storage");
  assert(copied.join(",") === "3,1,2", "the copy keeps the order");
  assert(wrap(["a", "b"]).join("") === "<ab>", "spread between elements");
  assert(fromTuple([1, "a"]) === 2, "a tuple parameter spreads");
  assert(fromReadonly([1, 2]) === 2, "a readonly parameter spreads");
}
