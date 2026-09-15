interface Point {
  x: number;
}

function main(): void {
  const xs: number[] = [];
  assert(!!xs, "empty array is truthy");
  const p: Point = { x: 0 };
  assert(!!p, "object is truthy");
  const bytes: Uint8Array = new Uint8Array([]);
  assert(!!bytes, "empty Uint8Array is truthy (JS quirk)");
  const f = (): number => 1;
  assert(!!f, "closure is truthy");
}
