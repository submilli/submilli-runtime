// Where control flow joins, the joined state keeps what every incoming path
// narrows, as in TypeScript: a loop head joins the entry with each back edge,
// an `||` whose right side cannot hold keeps its left side's narrowing, and a
// module `let`'s initializer or a top-level assignment narrows it for the
// top-level statements after it.
function size(s: string | number): number {
  return typeof s === "string" ? s.length : s + 100;
}

function loopHead(n: number): number {
  let x: string | number | boolean = true;
  x = "abc";
  let i = 0;
  while (i < n) {
    x = size(x);
    i++;
  }
  const after: string | number = x;
  return typeof after === "string" ? after.length : after;
}

function nestedLoopHead(n: number): number {
  let x: string | number | boolean = "ab";
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < 2; j++) {
      x = size(x);
    }
  }
  const after: string | number = x;
  return typeof after === "string" ? after.length : after;
}

function doLoop(n: number): number {
  let x: string | number | boolean = "ab";
  do {
    x = size(x);
    n--;
  } while (n > 0);
  const after: number = x;
  return after;
}

function repeatedGuard(x: string | number): number {
  return typeof x === "string" || typeof x === "string" ? x.length : 0;
}

function negatedConjunction(x: string | number): number {
  if (!(typeof x === "number" && typeof x === "number")) {
    return x.length;
  }
  return x;
}

let moduleValue: string | number = 1;
moduleValue = "four";
const moduleLength = moduleValue.length;
let moduleInitialized: number | string = "five";
const initializedLength = moduleInitialized.length;

function main(): void {
  assert(loopHead(0) === 3 && loopHead(1) === 3 && loopHead(3) === 203, "a while head joins its back edge");
  assert(nestedLoopHead(0) === 2 && nestedLoopHead(2) === 302, "nested loop heads join their back edges");
  assert(doLoop(1) === 2 && doLoop(3) === 202, "a do-while body joins its back edge");
  assert(repeatedGuard("xyz") === 3 && repeatedGuard(4) === 0, "a repeated guard narrows");
  assert(negatedConjunction("xy") === 2 && negatedConjunction(7) === 7, "a negated conjunction narrows");
  assert(moduleLength === 4, "a top-level assignment narrows a module `let`");
  assert(initializedLength === 4, "so does its initializer");
}
