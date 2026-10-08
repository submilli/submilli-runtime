// An argument must fit the method's signature on every member, as in tsc:
// `includes` on `string | number[]` takes neither a number nor a string.
// expect-error: expected `string`, got `number`
// expect-error: expected `number`, got `string`
// expect-error-count: 2
function main(): void {
  const u: string | number[] = [1].length > 0 ? [1] : "1";
  const a = u.includes(1);
  const b = u.includes("1");
}
