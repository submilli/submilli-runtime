// A narrowing that holds on the back edge is not a fact about loop *entry*:
// iteration 1 doesn't arrive that way. Installing it at entry made codegen
// materialize a region from a source the frame hadn't assigned yet
// (`uninitialized local`), and — where it didn't trap — accepted reads that are
// null on the first pass.
//
// The four loop forms are spelled out because each reaches the fixed point
// through a different desugar.

function whileForm(seed: string | null): number {
  let x: string | null = seed;
  let n = 0;
  while (n < 3) {
    n = n + 1;
    x = "zz";
    continue;
  }
  return x === null ? 0 : x.length;
}

function forForm(seed: string | null): number {
  let x: string | null = seed;
  for (let n = 0; n < 3; n = n + 1) {
    x = "zz";
    continue;
  }
  return x === null ? 0 : x.length;
}

function forOfForm(seed: string | null): number {
  let x: string | null = seed;
  for (const _ of [1, 2, 3]) {
    x = "zz";
    continue;
  }
  return x === null ? 0 : x.length;
}

// No `continue` here: a `do`/`while` lowers to `while (true) { body; if (!c)
// break; }`, so a `continue` in the body skips the check and spins forever
// (SUB-824). The natural back edge exercises the same entry-env rule.
function doWhileForm(seed: string | null): number {
  let x: string | null = seed;
  let n = 0;
  do {
    n = n + 1;
    x = "zz";
  } while (n < 3);
  return x === null ? 0 : x.length;
}

// Every back edge is a `continue` behind a guard — nothing is assigned, and the
// entry state still must not inherit the guard's conclusion.
function guardedBackEdge(x: string | null): string {
  let out = "";
  let n = 0;
  do {
    n = n + 1;
    if (n >= 3) {
      break;
    }
    if (x === null) {
      break;
    }
    out = out + x;
    continue;
  } while (true);
  return out;
}

// The back edge joins to a two-member union rather than a single type; the
// entry region used to cast `string | number | null` down on iteration 1.
function unionBackEdge(x: string | number | null): string {
  let out = "";
  let n = 0;
  while (n < 2) {
    n = n + 1;
    if (x === null) {
      break;
    }
    if (typeof x === "string") {
      out = out + x;
      continue;
    }
    out = out + x.toString();
    continue;
  }
  return out;
}

function main(): void {
  assert(whileForm(null) === 2, "while: assignment on the back edge, null seed");
  assert(whileForm("a") === 2, "while: non-null seed");
  assert(forForm(null) === 2, "for");
  assert(forOfForm(null) === 2, "for-of");
  assert(doWhileForm(null) === 2, "do-while");

  assert(guardedBackEdge("a") === "aa", "guarded continue, non-null");
  assert(guardedBackEdge(null) === "", "guarded continue, null");

  assert(unionBackEdge(null) === "", "union back edge, null");
  assert(unionBackEdge("s") === "ss", "union back edge, string");
  assert(unionBackEdge(3) === "33", "union back edge, number");
}
