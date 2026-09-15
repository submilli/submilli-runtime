// expect-error: the receiver can be `null`
function test(x: string | null, fail: boolean): number {
 if (x !== null) {
  try { if (fail) { x = null; throw new Error("bad"); } }
  catch(e) { return x.length; }
 }
 return 0;
}
function main(): void { console.log(test("ok", true)); }
