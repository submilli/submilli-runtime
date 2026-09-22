// expect-error: tuple literal has 3 elements, but type expects 2
function main(): void {
  const pair: [number, number] = [1, 2];
  const wrong: [number, number] = [...pair, 3];
}
