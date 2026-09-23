function f(k: number): number {
 switch(k) {
  case 1: try { return 1; } finally { break; }
  default: return 2;
 }
 return 3;
}
function main(): void { assert(f(1) === 3, "finally break cancels return"); assert(f(2) === 2, "ordinary return"); }
