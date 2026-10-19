// `object` holds no primitive, `null` or `undefined`, and has no fields of its
// own, as in TypeScript.
// expect-error: expected `object`, got `number`
// expect-error: expected `object`, got `string`
// expect-error: expected `object`, got `null`
// expect-error: expected `{ a: number }`, got `object`
// expect-error-count: 4
function main(): void {
  const n: object = 1;
  const s: object = "s";
  const z: object = null;
  const o: object = {};
  const shaped: { a: number } = o;
}
