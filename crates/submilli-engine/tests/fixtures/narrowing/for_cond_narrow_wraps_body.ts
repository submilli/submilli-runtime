// A `for` condition that narrows makes the loop fixed point wrap the body in a
// NarrowRegion, so the body the desugarer sees is no longer a Block. Both
// lowering shapes must accept that: the plain `while` (no update clause) and the
// `if (cond) { body } else { break; }` guard the update-clause shape builds.

interface Node {
  value: number;
  next: Node | null;
}

function countDown(start: number): number {
  let sum = 0;
  let passes = 0;
  for (let i: number | null = start; i !== null; ) {
    sum = sum + i;
    passes = passes + 1;
    i = i > 1 ? i - 1 : null;
  }
  return sum * 10 + passes;
}

// `passes++` reaches the update slot as a `PostfixUnary`, lowered by the pass that
// runs *after* the loop desugarers — so the wrapper has to survive that too.
function postfixUpdate(): number {
  let passes = 0;
  let chars = 0;
  for (let s: string | null = "abc"; s !== null; passes++) {
    chars = chars + s.length;
    s = passes >= 2 ? null : s;
  }
  return passes * 100 + chars;
}

function withUpdate(): number {
  let passes = 0;
  let chars = 0;
  for (let s: string | null = "abc"; s !== null; passes = passes + 1) {
    chars = chars + s.length;
    s = passes >= 2 ? null : s;
  }
  return passes * 100 + chars;
}

function continueRunsUpdate(): number {
  let steps = 0;
  for (let v: number | null = 0; v !== null; steps = steps + 1) {
    if (steps >= 2) {
      v = null;
      continue;
    }
    v = v + 1;
  }
  return steps;
}

function nested(): number {
  let hits = 0;
  for (let outer: string | null = "ab"; outer !== null; outer = null) {
    for (let inner: number | null = outer.length; inner !== null; ) {
      hits = hits + inner;
      inner = null;
    }
  }
  return hits;
}

// The narrowed path is a field chain, so the region materializes a shadow for
// `head.next` rather than for a bare identifier.
function fieldPath(head: Node): number {
  let total = 0;
  for (let guard: number | null = 1; head.next !== null; guard = null) {
    total = total + head.next.value;
    head.next = null;
    if (guard === null) {
      break;
    }
  }
  return total;
}

// Every back edge is a `continue` carrying a narrowing, so the fixed point
// reruns the body and wraps it even though the condition narrows nothing.
function backEdge(x: string | null): string {
  let out = "";
  for (let i = 0; i < 2; i = i + 1) {
    if (x === null) {
      out = out + "z";
      continue;
    }
    out = out + x;
    continue;
  }
  return out;
}

function main(): void {
  assert(countDown(3) === 63, "3 + 2 + 1 over three passes");
  assert(countDown(0) === 1, "start below 1 still runs the body once");
  assert(postfixUpdate() === 309, "postfix update clause under a wrapper");
  assert(withUpdate() === 309, "three updates, three passes of length 3");
  assert(continueRunsUpdate() === 3, "continue still advances the update clause");
  assert(nested() === 2, "inner loop sees the outer narrowing");

  const head: Node = { value: 1, next: { value: 7, next: null } };
  assert(fieldPath(head) === 7, "field-path narrowing in a for condition");

  assert(backEdge("-") === "--", "narrowed back edge, non-null");
  assert(backEdge(null) === "zz", "narrowed back edge, null");
}
