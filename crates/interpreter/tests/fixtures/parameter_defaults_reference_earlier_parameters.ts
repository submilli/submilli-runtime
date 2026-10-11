enum E { A = 1, B = 2 }
function plain(a: number, x: number = a): number { return x; }
function shadowsGlobal(Infinity: number, x: number = Infinity): number { return x; }
function shadowsNegatedGlobal(NaN: number, x: number = -NaN): number { return x; }
function shadowsEnum(E: { B: number }, x: number = E.B): number { return x; }
function main(): void {
  assert(plain(3) === 3, "default reads earlier parameter");
  assert(shadowsGlobal(4) === 4, "parameter shadows Infinity");
  assert(shadowsNegatedGlobal(5) === -5, "parameter shadows NaN under negation");
  assert(shadowsEnum({ B: 7 }) === 7, "parameter shadows enum value");
}
