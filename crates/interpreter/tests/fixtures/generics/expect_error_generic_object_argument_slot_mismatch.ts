// A value that doesn't fit what an earlier field of the same object literal
// argument bound is reported at that value, as tsc reports it, once for each
// such field.
// expect-error: expected `number`, got `string`
// expect-error: expected `number`, got `boolean`
// expect-error: expected `number`, got `number[]`
// expect-error-count: 3
function three<T>(o: { v: T; w: T; x: T; cb: (t: T) => string }): T {
  return o.v;
}

function common<T>(o: { v: T; cb: (t: T) => string; w: T }): T {
  return o.w;
}

function main(): void {
  const mixed = three({ v: 1, w: "s", x: true, cb: (t) => `${t}` });
  const list = common({ v: 1, cb: (t) => `${t}`, w: [1] });
}
