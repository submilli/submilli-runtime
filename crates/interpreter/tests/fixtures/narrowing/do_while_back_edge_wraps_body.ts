// A `do`/`while` has no entry narrowing — the condition runs after the body —
// but a `continue` carrying one is a back edge, so the loop fixed point reruns
// the body and wraps it in a NarrowRegion. The desugar puts the condition check
// at the top of the loop and must nest the wrapped body whole beneath it instead
// of assuming a Block.

function run(x: string | null): string {
  let out = "";
  let n = 0;
  do {
    n = n + 1;
    if (n >= 3) {
      break;
    }
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  } while (true);
  return out;
}

// Wrapped *and* shadowing, so one function pins both do-while invariants: the
// wrapper nests whole, and the body's block survives so `go > 3` still reads the
// outer number rather than the body's string. Splicing either open breaks this.
function bothInvariants(x: string | null): string {
  let go = 5;
  let out = "";
  let n = 0;
  do {
    const go = "in";
    n = n + 1;
    if (n >= 3) {
      break;
    }
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  } while (go > 3);
  return out + "|" + n.toString();
}

// The `continue` back edge reaches a real condition test — the test is what ends
// this loop, so a narrowing on the back edge has to survive alongside it. The
// `n > 10` bail is what keeps a regression here a failed assertion instead of a
// loop that runs until the heap cap: if `continue` ever skips the test again,
// nothing else stops this.
function narrowedContinueHitsTest(x: string | null): string {
  let out = "";
  let n = 0;
  do {
    n = n + 1;
    if (n > 10) {
      return "ran away";
    }
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  } while (n < 3);
  return out;
}

function main(): void {
  assert(run("a") === "aa", "narrowed back edge, non-null");
  assert(run(null) === "zz", "narrowed back edge, null");
  assert(bothInvariants("-") === "--|3", "wrapped and shadowing, non-null");
  assert(bothInvariants(null) === "zz|3", "wrapped and shadowing, null");
  assert(narrowedContinueHitsTest("-") === "---", "continue reaches the test, non-null");
  assert(narrowedContinueHitsTest(null) === "zzz", "continue reaches the test, null");
}
