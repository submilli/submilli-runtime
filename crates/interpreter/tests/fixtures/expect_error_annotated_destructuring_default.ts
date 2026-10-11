// expect-error: expected `number | undefined`, got `string`
// expect-error-count: 2
// An annotated pattern's default must fit the declared component, as in
// TypeScript; an unannotated one joins it instead.
function main(): void {
  const { a = "z" }: { a?: number } = {};
  const [b = "q"]: (number | undefined)[] = [];
  const { c = "y" } = { c: 1 as number | undefined };
  const joined: string | number = c;
}
