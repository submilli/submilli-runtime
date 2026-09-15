// `continue` in a `do`/`while` jumps to the condition test, not past it
// (ECMA-262 §14.7.2). The test therefore runs at the top of every pass but the
// first, so a bare `continue` still terminates the loop.
function main(): void {
  let n = 0;
  let iters = 0;
  do {
    n = n + 1;
    iters = iters + 1;
    if (n < 5) {
      continue;
    }
  } while (n < 3);
  assert(n === 3, "the skipped-over test still exits the loop");
  assert(iters === 3, "one pass per test, not five");

  // With no other exit, the test is the only thing that ends the loop.
  let m = 0;
  let out = "";
  do {
    m = m + 1;
    out = out + "x";
    continue;
  } while (m < 2);
  assert(out === "xx", "a bare `continue` does not loop forever");

  // A `continue` that unwinds a `try`/`finally` runs the finally before the test.
  let order = "";
  let k = 0;
  do {
    k = k + 1;
    try {
      order = order + "t";
      continue;
    } finally {
      order = order + "f";
    }
  } while (k < 3);
  assert(order === "tftftf", "finally runs on each continue");
  assert(k === 3, "and the test still terminates the loop");

  // `continue` in an inner do/while targets the inner loop only, and its last
  // pass continues into a test that is *false* — so the test is what ends it.
  // A `continue` landing on an already-true test would prove nothing here.
  let log = "";
  let i = 0;
  do {
    i = i + 1;
    let j = 0;
    do {
      j = j + 1;
      log = log + i.toString() + j.toString();
      continue;
    } while (j < 2);
  } while (i < 2);
  assert(log === "11122122", "inner continue does not escape to the outer loop");
  assert(i === 2, "outer loop ran twice");

  // The condition is evaluated once per pass after the first, `continue` included.
  let z = 0;
  do {
    z = z + 1;
    continue;
  } while (sideEffectingCond());
  assert(z === 3, "three passes");
  assert(calls === 3, "three condition evaluations, one per continue");

  // `continue` from inside a `catch` reaches the test.
  let cout = "";
  let ci = 0;
  do {
    ci = ci + 1;
    try {
      throw new Error("e");
    } catch (e: Error) {
      cout = cout + "c";
      continue;
    }
  } while (ci < 3);
  assert(cout === "ccc" && ci === 3, "continue from a catch reaches the test");

  // Inside a `switch`, `break` binds to the switch and `continue` to the loop —
  // and the `continue` lands on a test that is false, so it is the loop's exit.
  let sout = "";
  let si = 0;
  do {
    si = si + 1;
    switch (si) {
      case 1:
        sout = sout + "one";
        break;
      default:
        sout = sout + "two";
        continue;
    }
    sout = sout + "!";
  } while (si < 2);
  assert(sout === "one!two", "break binds to the switch, continue to the loop");

  // A body-scoped binding shadowing a name the condition reads never reaches the head.
  let limit = 3;
  let shadowed = "";
  let sn = 0;
  do {
    let limit = "body";
    shadowed = shadowed + limit;
    sn = sn + 1;
  } while (sn < limit);
  assert(shadowed === "bodybodybody" && sn === 3, "the head reads the outer binding");

  // Two loops in sequence get distinct first-pass flags.
  let x1 = 0;
  do {
    x1 = x1 + 1;
    continue;
  } while (x1 < 2);
  let x2 = 0;
  do {
    x2 = x2 + 1;
    continue;
  } while (x2 < 4);
  assert(x1 === 2 && x2 === 4, "each do/while gets its own first-pass flag");

  // `break` still leaves without consulting the test.
  let b = 0;
  do {
    b = b + 1;
    if (b === 2) {
      break;
    }
    continue;
  } while (b < 10);
  assert(b === 2, "break exits regardless of the condition");
}

let calls: number = 0;

function sideEffectingCond(): boolean {
  calls = calls + 1;
  return calls < 3;
}
