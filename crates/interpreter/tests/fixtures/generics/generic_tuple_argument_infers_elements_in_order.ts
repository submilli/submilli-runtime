// A generic call's tuple literal argument types each element with what the
// elements before it bound, as tsc's intra-expression inference does, so a
// callback element reads the type parameter an earlier element fixed.
function callIt<T>(obj: [(n: number) => T, (x: T) => string]): string {
  return obj[1](obj[0](2));
}

function pairUp<T>(obj: [T, (x: T) => number]): number {
  return obj[1](obj[0]);
}

function main(): void {
  const fixed = callIt([() => 1.5, (n) => n.toFixed(1)]);
  const fromParam = callIt([(a) => a * 2, (n) => n.toFixed(0)]);
  const length = pairUp(["abc", (x) => x.length]);
  assert(fixed === "1.5" && fromParam === "4" && length === 3, "tuple elements in order");
}
