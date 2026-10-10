let left: number | string = 0;
let right: number | bigint = 20;
function change(): boolean { left = "1_0"; right = 20n; return false; }
function compare(): boolean {
  if (typeof left !== "number" || typeof right !== "number" || change()) return false;
  return left < right;
}
function main(): void { assert(compare() === false); }
