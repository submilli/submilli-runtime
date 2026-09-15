// The point of the `string | null` return: an out-of-range read can no longer
// be used as a string without narrowing it first.
// expect-error: expected `string`, got `string | null`
function main(): void {
  const s = "abc";
  const c: string = s.at(0);
}
