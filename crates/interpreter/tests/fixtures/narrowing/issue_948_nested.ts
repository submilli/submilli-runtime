class K { v: number = 1; }
class L extends K { w: number = 2; }
function nested<T>(x:T):T {
 if (x instanceof K) {
   if (x instanceof L) { assert(x.w === 2); return x; }
 }
 return x;
}
export function main(): void { assert(nested(new L()).v === 1); }
