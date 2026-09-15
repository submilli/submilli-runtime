function test(x: string | null): number {
 if (x !== null) {
  let n = 0;
  for (; x.length > 0; n += 0) { n += 1; x = ""; }
  return n;
 }
 return 0;
}
function main(): void { assert(test("ok") === 1, "condition reads current string"); }
