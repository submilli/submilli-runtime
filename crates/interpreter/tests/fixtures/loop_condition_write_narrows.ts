// A `while` or `for` condition runs again before every pass, so a write in it
// narrows the body on every pass, and the loop head sees the state each back
// edge returns with: after the body, and after a `for` update.

function main(): void {
  let x: string | number = 0;
  x = 5;
  let passes = 0;
  while ((x = x.toFixed()) !== "8") {
    const s: string = x;
    assert(s.length === 1, "the condition's write narrows the body");
    passes++;
    x = 5 + passes;
  }
  const last: string = x;
  assert(last === "8" && passes === 3, "the exit sees the condition's write");

  let y: string | number | boolean = true;
  y = true;
  let seen = "";
  for (y = 5; y = y.toFixed(); y = 5) {
    const s: string = y;
    seen = seen + s;
    break;
  }
  assert(seen === "5", "a `for` update reaches the condition");

  let z: string | number = "a";
  z = "a";
  let reads = 0;
  while (z.length > 0 && (z = 5) === 5) {
    reads++;
    z = reads < 3 ? "b" : "";
  }
  assert(reads === 3, "a read before the write sees what the body left");

  let w: string | number = 0;
  w = 1;
  let kept = 0;
  while ((w = w.toFixed()) !== "4") {
    if (kept === 1) {
      kept++;
      w = 3;
      continue;
    }
    kept++;
    w = kept;
  }
  assert(kept === 4, "a `continue` returns to the condition");

  let v: string | null = "s";
  v = "s";
  let polls = 0;
  do {
    polls++;
    v = polls < 3 ? "more" : null;
  } while (v !== null);
  const gone: null = v;
  assert(gone === null && polls === 3, "a `do … while` exits where its condition fails");

  let u: string | number | null = "a";
  u = "a";
  let steps = 0;
  while (u !== null && steps < 2) {
    const kept: string | number = u;
    assert(kept !== null, "a guard widens to what the body leaves, not to its declaration");
    u = 5;
    steps++;
  }

  let state: "a" | "b" | "c" | null = "a";
  state = "a";
  let trail = "";
  while (state !== null) {
    const current: "a" | "b" | "c" = state;
    trail = trail + current;
    state = current === "a" ? "b" : current === "b" ? "c" : null;
  }
  assert(trail === "abc", "a state machine keeps its non-null states");
}
