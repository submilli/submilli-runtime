// expect-error: does not satisfy
function main(): void {
  const x: FlatArray<number[][], string> = 1;
}
