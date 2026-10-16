// `a?: string` and `a?: string | undefined` are one type, so spreading either
// over an earlier `a` gives `a` the earlier type or `string`, as in TypeScript.
function main(): void {
  const b: { a?: string } = {};
  const d: { a?: string | undefined } = { a: "d" };
  const u: { a?: undefined } = {};
  const b1: { a: number | string } = { a: 123, ...b };
  const d1: { a: number | string } = { a: 123, ...d };
  const u1: { a: number } = { a: 123, ...u };
  assert(b1.a === 123, "an absent field keeps the earlier value");
  assert(d1.a === "d", "a present field replaces it");
  assert(u1.a === 123, "a field that can only be undefined adds nothing");
}
