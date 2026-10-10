// The point of the `string | undefined` return: an out-of-range read can no longer
// be used as a string without narrowing it first.
// expect-error: expected `string`, got `string | undefined`
function main(): void {
  const s = "abc";
  const c: string = s.at(0);
}
