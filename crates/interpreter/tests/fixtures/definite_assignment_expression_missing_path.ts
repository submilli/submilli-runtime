// expect-error: variable `ternary` is used before being assigned
// expect-error: variable `and` is used before being assigned
// expect-error: variable `or` is used before being assigned
// expect-error: variable `invoked` is used before being assigned
function check(flag: boolean): void {
  let ternary: number;
  flag ? (ternary = 1) : 2;
  console.log(ternary);
  let and: number;
  flag && (and = 3);
  console.log(and);
  let or: number;
  flag || (or = 4);
  console.log(or);
  let invoked: number;
  (() => { if (flag) { return; } invoked = 5; })();
  console.log(invoked);
}
function main(): void { check(false); }
