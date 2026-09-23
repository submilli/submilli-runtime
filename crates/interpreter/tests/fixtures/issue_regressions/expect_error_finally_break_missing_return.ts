// expect-error: function `f` does not return a value on all paths
function f(k: number): number {
 switch(k) {
  case 1: try { return 1; } finally { break; }
  default: return 2;
 }
}
function main(): void { console.log(f(1)); }
