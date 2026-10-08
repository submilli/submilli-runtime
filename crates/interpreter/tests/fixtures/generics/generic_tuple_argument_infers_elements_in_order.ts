// A generic call's tuple literal argument types its callback elements after
// its other elements, each with what the elements before it bound, as tsc's
// intra-expression inference does, so a callback element reads the type
// parameter another element fixed.
function callIt<T>(obj: [(n: number) => T, (x: T) => string]): string {
  return obj[1](obj[0](2));
}

function pairUp<T>(obj: [T, (x: T) => number]): number {
  return obj[1](obj[0]);
}

function reversed<T>(obj: [(x: T) => number, T]): number {
  return obj[0](obj[1]);
}

function main(): void {
  const fixed = callIt([() => 1.5, (n) => n.toFixed(1)]);
  const fromParam = callIt([(a) => a * 2, (n) => n.toFixed(0)]);
  const length = pairUp(["abc", (x) => x.length]);
  const first = reversed([(x) => x.length, "abcd"]);
  assert(fixed === "1.5" && fromParam === "4" && length === 3, "tuple elements in order");
  assert(first === 4, "callback element before its value");
}
