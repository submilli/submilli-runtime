// expect-error: `Color` is an enum type, not a value; use `Color.<variant>`
// expect-error: variants: `Color.Red`, `Color.Blue`
// expect-error: no variant `Green` on enum `Color`
// expect-error: enum member `Color.Red` is not a generic type
// expect-error: `Color` is an enum type, not a value, so `typeof` has no type to take
// expect-error: unresolved identifier `Color`
// expect-error: `instanceof` requires a class on the right-hand side, but `Color` is not a class
// expect-error: `instanceof` requires a class on the right-hand side, but `Point` is not a class
// Diagnostics about a local enum or interface use its source name.

function main(): void {
  enum Color { Red, Blue }
  interface Point { x: number; }
  const all = Object.values(Color);
  const green: Color.Green = Color.Red;
  const generic: Color.Red<number> = Color.Red;
  const kind: typeof Color = Color.Red;
  const p: Point = { x: 1 };
  Color = 2;
  const isColor = p instanceof Color;
  const isPoint = p instanceof Point;
}
