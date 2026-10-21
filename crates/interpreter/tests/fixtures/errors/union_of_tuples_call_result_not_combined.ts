// expect-error: expected `number[]`, got `string[]`
// Only an array literal's tuple members form the union of their array
// candidates. A call's union of tuples infers from its first member, which
// the second then fails to fit, as tsc reports.
function first<A, B>(t: [A, B]): A {
  return t[0];
}
function make(): [number[], string] | [string[], string] {
  return [[1], "a"];
}

function main(): void {
  console.log(first(make()));
}
