// SUB-1759: `Boolean(x)` converts any value by its truthiness, as in tsc and
// JavaScript, and `Boolean` is a value like `String` and `Number`.
function main(): void {
  assert(Boolean(1) && Boolean("a") && Boolean([]) && Boolean({}) && Boolean(1n));
  assert(!Boolean(0) && !Boolean(NaN) && !Boolean("") && !Boolean(0n));
  assert(!Boolean(null) && !Boolean(undefined) && !Boolean());
  const maybe: string | null = null;
  const b: boolean = Boolean(maybe);
  assert(!b);
  const words = ["a", "", "b"];
  assert(words.filter((w: string): boolean => Boolean(w)).join(",") === "a,b");
}
