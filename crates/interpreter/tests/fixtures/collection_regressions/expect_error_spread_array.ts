// expect-error: tuple literal spread requires a fixed-length tuple
function main(): void {
  const array = [1, 2];
  const pair: [number, number] = [...array];
}
