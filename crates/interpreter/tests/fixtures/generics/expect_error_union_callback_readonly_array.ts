// A callback of an array union's method sees the array readonly: pushing a
// member of one array type into the other is rejected (tsc rejects it too).
// expect-error: cannot call `push` on `readonly (number | string)[]`
// expect-error-count: 1
function main(): void {
  const flag: boolean = true;
  const nums: number[] = [1];
  const u: number[] | string[] = flag ? nums : ["a"];
  u.forEach((v, i, arr) => { if (i === 0) arr.push("x"); });
}
