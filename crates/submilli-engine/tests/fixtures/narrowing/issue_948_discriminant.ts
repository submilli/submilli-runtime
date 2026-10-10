class K { kind: "k" = "k"; v: number = 1; }
class L { kind: "l" = "l"; w: number = 2; }
function isKL(x: unknown): x is K | L { if (x instanceof K) { return true; } return x instanceof L; }
function keep<T>(x:T):T {
 if (isKL(x)) {
  if (x.kind === "k") { assert(x.v === 1); return x; }
  return x;
 }
 return x;
}
function keepSwitch<T>(x:T):T {
 if (isKL(x)) {
  switch (x.kind) { case "k": assert(x.v === 1); return x; default: assert(x.w === 2); return x; }
 }
 return x;
}
export function main(): void {
 assert(keep(new K()).v === 1); assert(keep(new L()).w === 2);
 assert(keepSwitch(new K()).v === 1); assert(keepSwitch(new L()).w === 2);
}
