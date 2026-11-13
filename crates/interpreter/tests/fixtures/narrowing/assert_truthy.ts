// `asserts x`: after the call, the argument is truthy.
function assertDefined<T>(x: T | null | undefined): asserts x {
  if (x === null || x === undefined) throw new Error("missing");
}

function length(s: string | undefined): number {
  assertDefined(s);
  return s.length;
}

function main(): void {
  const maybe: { name: string } | null = { name: "a" };
  assertDefined(maybe);
  assert(maybe.name === "a", "null removed");
  assert(length("abc") === 3, "undefined removed");
}
