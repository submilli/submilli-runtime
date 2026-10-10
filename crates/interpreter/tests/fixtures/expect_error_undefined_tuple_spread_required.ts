// expect-error: tuple literal has 1 to 2 elements
function copy(value: [number, string?]): [number, string | undefined] {
  return [...value];
}
function main(): void {}
