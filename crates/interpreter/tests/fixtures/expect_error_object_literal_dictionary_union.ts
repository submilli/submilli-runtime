// A field no member of the union accepts is reported against every type a
// member gives its name (TS2322 in TypeScript).
// expect-error: expected `number | string`, got `boolean`
function show(o: { [k: string]: string } | { a: number }): string {
  return JSON.stringify(o);
}

function main(): void {
  console.log(show({ a: true }));
}
