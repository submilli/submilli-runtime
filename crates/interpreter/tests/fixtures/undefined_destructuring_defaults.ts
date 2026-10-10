// A destructuring default takes the component's type as context, in `const`
// and `let` as in parameters: `[]` is a `string[]` and `{}` an `{ m?: string }`.
interface Opts { l?: string[]; n?: { m?: string }; k?: number; }
function describe(o: Opts): string {
  const { l = [], n = {}, k = 3 } = o;
  const { m } = n;
  let { l: copy = [] } = o;
  copy = ["x"];
  return `${l.length} ${m} ${k} ${copy.length}`;
}
function main(): void {
  assert(describe({}) === "0 undefined 3 1", "defaults apply to omitted members");
  assert(describe({ l: ["a", "b"], n: { m: "M" }, k: 1 }) === "2 M 1 1", "present members win");
}
