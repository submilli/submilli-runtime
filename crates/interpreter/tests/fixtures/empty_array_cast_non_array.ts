// A cast gives an empty `[]` its element type only when the target has one; a
// tuple target has fixed positions an empty literal can't fill.
// Each line reports the empty array and then the cast it fails, and nothing
// from the target being used as a hint.
// expect-error: cannot infer element type of empty array
// expect-error: cannot cast `<error>[]` to `string`
// expect-error-count: 6
function main(): void {
  const s = [] as string;
  const n = [] as number;
  const t = [] as [number] | null;
}
