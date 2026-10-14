// A template whose substitutions are constant bitwise, shift or exponent
// expressions is the string literal they spell, as tsc evaluates them.
function main(): void {
  const and: "0" = `${3 & 4}`;
  const spelled: "205" = `2${3 & 4}5`;
  const or: "7" = `${3 | 4}`;
  const xor: "6" = `${5 ^ 3}`;
  const not: "-4" = `${~3}`;
  const shl: "-2147483648" = `${1 << 31}`;
  const count: "2" = `${1 << 33}`;
  const shr: "-1" = `${-1 >> 4}`;
  const ushr: "268435455" = `${-1 >>> 4}`;
  const wide: "4294967295" = `${4294967295 >>> 0}`;
  const pow: "8" = `${2 ** 3}`;
  const folded = [and, spelled, or, xor, not, shl, count, shr, ushr, wide, pow].join(",");
  assert(folded === "0,205,7,6,-4,-2147483648,2,-1,268435455,4294967295,8", folded);
}
