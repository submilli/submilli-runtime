// expect-error: expected `1 | undefined`
function main(): void {
  [[[1]]].flat<number[][][]>(2);
}
