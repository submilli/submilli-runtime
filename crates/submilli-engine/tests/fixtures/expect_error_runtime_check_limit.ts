// expect-error: exceeds the compiler limit of 32768 steps
// A runtime check of an interface is emitted inline with its members' checks,
// and each interface here checks the one below it twice, so the code for a
// check doubles per level. The limit is reached inside the check of an inner
// interface, emitted as a validator function of its own.
interface I0 { v: number }
interface I1 { a: I0; b: I0 }
interface I2 { a: I1; b: I1 }
interface I3 { a: I2; b: I2 }
interface I4 { a: I3; b: I3 }
interface I5 { a: I4; b: I4 }
interface I6 { a: I5; b: I5 }
interface I7 { a: I6; b: I6 }
interface I8 { a: I7; b: I7 }
interface I9 { a: I8; b: I8 }
interface I10 { a: I9; b: I9 }
interface I11 { a: I10; b: I10 }
interface I12 { a: I11; b: I11 }
interface I13 { a: I12; b: I12 }
interface I14 { a: I13; b: I13 }
interface I15 { a: I14; b: I14 }
function f(x: I15 | null): number {
  return x === null ? 0 : 1;
}
export function main(): number {
  return f(null);
}
