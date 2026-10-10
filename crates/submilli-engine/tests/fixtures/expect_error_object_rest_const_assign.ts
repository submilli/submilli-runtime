// expect-error: cannot assign to const binding `rest`
function main(): void {
  const source = { a: 1, b: 2 };
  const { a, ...rest } = source;
  rest = { b: 3 };
}
