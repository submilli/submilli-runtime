// A cast gives an empty `[]` its element type only when the target has one; a
// tuple target has fixed positions an empty literal can't fill. Otherwise the
// literal is `never[]`, which no such target relates to, as in tsc.
// expect-error: cannot cast `never[]` to `string`
// expect-error: cannot cast `never[]` to `number`
// expect-error: cannot cast `never[]` to `null | [number]`
// expect-error-count: 3
function main(): void {
  const s = [] as string;
  const n = [] as number;
  const t = [] as [number] | null;
}
