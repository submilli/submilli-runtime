// A spread keeps an index signature every source has with the same key, with
// the union of their values (tsc's spread rule).
function main(): void {
  const a: Record<`a${string}`, number> = { a1: 1 };
  const more: Record<`a${string}`, string> = { a2: "two" };
  const spread = { ...a, ...more };
  const first: number | string = spread.a1;
  const second: number | string = spread.a2;
  assert(first === 1 && second === "two", "same-key spread keeps the signature");
  const copy = { ...a };
  const only: number = copy.a1;
  assert(only === 1, "a lone spread keeps its signatures");
}
