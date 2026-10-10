// A literal typed as an interface checks each field against the interface's
// type, including a field that comes from a spread (TS2322 in TypeScript).
// expect-error: spread field `a`: expected `number`, got `string`
interface Point {
  a: number;
}

function main(): void {
  const s: { a: string } = { a: "x" };
  const p: Point = { a: 1, ...s };
  console.log(p.a);
}
