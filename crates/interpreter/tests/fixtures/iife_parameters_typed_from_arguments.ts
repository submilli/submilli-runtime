// A function literal called in place types each unannotated parameter from
// the argument passed to it, widened, as tsc does; a default is then typed
// against that parameter.
function pick(): string | number {
  return "ab";
}

function main(): void {
  const picked = pick();
  if (typeof picked === "string") {
    assert(((s) => s.length)(picked) === 2, "a narrowed argument");
  }

  const next = ((x) => x + 1)(2);
  assert(next === 3, "a number argument");

  const length = (function (s) {
    return s.length;
  })("abc");
  assert(length === 3, "a function expression");

  const named = (function measure(s) {
    return s.length;
  })("abcd");
  assert(named === 4, "a named function expression");

  const deep = ((a) => a)(((b) => b)(((c) => c)(1)));
  assert(deep === 1, "nested in arguments");

  const r = (({ r = 17 } = { r: 18 }) => r)({ r: 19 });
  assert(r === 19, "the argument, not the default");

  const n = ((o) => o.n)({ n: 4, m: "x" });
  assert(n === 4, "an object argument");

  const count = ((xs) => xs.length)([1, 2, 3]);
  assert(count === 3, "an array argument");

  const sum = ((x: number, y) => x + y)(1, 2);
  assert(sum === 3, "beside an annotated parameter");

  const flag = ((t) => !t)(true);
  assert(flag === false, "a boolean argument");
}
