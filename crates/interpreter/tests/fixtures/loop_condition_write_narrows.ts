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
}
