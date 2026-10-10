// An argument must fit the method's signature on every member, as in tsc:
// `includes` on `string | number[]` takes neither a number nor a string. A
// wrong argument count is reported once, not once per member.
// expect-error: expected `string`, got `1`
// expect-error: expected `number`, got `"1"`
// expect-error: method `slice` expects 0-2 argument(s), got 3
// expect-error-count: 3
function main(): void {
  const u: string | number[] = [1].length > 0 ? [1] : "1";
  const a = u.includes(1);
  const b = u.includes("1");
  const c = u.slice(1, 2, 3);
}
