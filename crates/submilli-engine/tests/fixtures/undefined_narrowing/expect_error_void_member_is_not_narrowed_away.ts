// expect-error: cannot read field `x` on non-object type `void | P`
// expect-error: expected `number`, got `5 | void`
// expect-error-count: 2
// A `void` result may hold any value, since a value-returning function can be
// stored as a `void` one. So `typeof v !== "undefined"` keeps a `void` member,
// and `??` on a `void` left side keeps it beside the fallback; reading on
// would read whatever the function returned.
type P = { x: number };
function op(get: () => void): P | void {
  return get();
}
function main(): void {
  const o = op(() => 5);
  if (typeof o !== "undefined") console.log(o.x);
  const g: () => void = () => "s";
  const n: number = g() ?? 5;
}
