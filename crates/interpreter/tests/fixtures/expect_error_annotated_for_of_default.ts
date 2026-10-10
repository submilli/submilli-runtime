// expect-error: expected `number | undefined`, got `string`
// expect-error-count: 1
// A type annotation on a `for-of` head is a Submilli extension: tsc rejects
// it (TS2483). Where it is written, the pattern's defaults are checked against
// it, like an annotated declaration's.
function main(): void {
  const rows: [string, number?][] = [["a"]];
  for (const [k, v = "z"]: [string, number?] of rows) {
    console.log(`${k}${v}`);
  }
}
