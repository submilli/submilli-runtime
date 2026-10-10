// expect-error: expected
function copy(value: [number, string?]): [number, string | undefined] | [boolean] {
  return [...value];
}
function main(): void {}
