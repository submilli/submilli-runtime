// expect-error: expected `number[]`
function flatten(xs: number[][][], depth: number): number[] {
  return xs.flat(depth);
}
function main(): void {}
