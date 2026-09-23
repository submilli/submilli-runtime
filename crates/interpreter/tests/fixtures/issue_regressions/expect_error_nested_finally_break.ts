// expect-error: no fallthrough
function f(a: number, b: number): number {
 switch (a) {
 case 1:
   switch (b) {
   case 1: try { return 1; } finally { break; }
   default: return 2;
   }
 case 2: return 3;
 default: return 4;
 }
}
function main(): void { console.log(f(1, 1)); }
