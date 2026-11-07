function choose(flag: boolean): number {
  let value: number;
  flag ? (value = 1) : (value = 2);
  return value;
}
function invoke(flag: boolean): number {
  let value: number;
  (() => {
    if (flag) { value = 6; return; }
    value = 7;
  })();
  return value;
}
function main(): void {
  assert(choose(true) === 1, "ternary then assigns");
  assert(choose(false) === 2, "ternary else assigns");
  let invoked: number;
  (() => { invoked = 3; })();
  assert(invoked === 3, "immediately invoked assignment");
  let and: number;
  true && (and = 4);
  assert(and === 4, "always evaluated conjunction");
  let or: number;
  false || (or = 5);
  assert(or === 5, "always evaluated disjunction");
  assert(invoke(true) === 6, "IIFE early return joins assigned facts");
  assert(invoke(false) === 7, "IIFE fallthrough joins assigned facts");
  let final: number;
  (() => { try { return; } finally { final = 8; } })();
  assert(final === 8, "finally assigns on IIFE return");
}
