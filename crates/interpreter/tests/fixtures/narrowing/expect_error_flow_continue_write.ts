// expect-error: the receiver can be `null`
function test(x: string | null): number {
 if (x !== null) {
  let n = 0;
  while (n < 2) {
   console.log(x.length);
   n += 1;
   if (n === 1) { x = null; continue; }
  }
 }
 return 0;
}
function main(): void { console.log(test("ok")); }
