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

  const one = 1;
  const reassigned = ((x) => {
    x = 2;
    return x;
  })(one);
  assert(reassigned === 2, "a literal argument types its parameter widened");

  // Inside a loop that a later pass retypes, an argument takes the type the
  // last pass gives it.
  const looped = ((k: string) => {
    let y: string | number = 1;
    let out = k;
    for (let i = 0; i < 2; i++) {
      out += ((z) => String(z))(y);
      y = "ab";
    }
    return out;
  })("");
  assert(looped === "1ab", "an argument retyped by a loop pass");

  // A loop condition's argument takes the type of each pass too.
  const conditioned = ((z: number) => {
    let v: string | number = "a";
    while (((q) => q !== 1)(v)) {
      v = 1;
    }
    return v === 1 ? z : -1;
  })(0);
  assert(conditioned === 0, "a loop condition's argument");
}
