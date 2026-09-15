// expect-error: cannot read field `length` on non-object type `null`
function test(): number {
 let x: string | null = null;
 while (true) {
  try { x = "ok"; break; } finally { x = null; }
 }
 return x.length;
}
function main(): void { console.log(test()); }
