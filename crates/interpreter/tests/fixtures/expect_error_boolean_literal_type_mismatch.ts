// `boolean` is wider than `true`, as in TypeScript (TS2322).
// expect-error: expected `true`, got `boolean`

function main(): void {
  const b: boolean = Math.random() > 0.5;
  const t: true = b;
  console.log(t);
}
