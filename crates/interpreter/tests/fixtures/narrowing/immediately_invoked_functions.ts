// A function invoked where it is written runs right there, so TypeScript
// checks its body as inline code: narrowing at the call holds inside it, and
// what it writes holds after the call.
let shared: string | null = "shared";
if (shared === null) {
  throw new Error("unset");
}
const sharedTail = (() => shared.slice(1))();

let counter: number | null = null;
(function () {
  counter = 1;
})();
counter++;

function source(): string | null {
  return "abc";
}

function readsNarrowedLet(): string {
  let text = source();
  if (!text) {
    return "";
  }
  return (() => text.slice(1))();
}

function writeFlowsOut(): number {
  let n: number | null = null;
  (() => {
    n = 2;
  })();
  return n + 1;
}

function writeReplacesNarrowing(): string {
  let w: number | null = 3;
  if (w !== null) {
    (() => {
      w = null;
    })();
  }
  return w === null ? "null" : "number";
}

function earlyReturnKeepsDeclaredType(flag: boolean): string {
  let n: number | null = 1;
  (() => {
    if (flag) {
      n = null;
      return;
    }
    n = 5;
  })();
  return n === null ? "null" : (n + 1).toString();
}

function fieldNarrowing(box: { label: string | null }): number {
  if (box.label !== null) {
    return (() => box.label.length)();
  }
  return -1;
}

function writesInLoop(): number {
  let total: number | null = 0;
  for (let i = 0; i < 3; i++) {
    (() => {
      total = (total ?? 0) + i;
    })();
  }
  return total ?? -1;
}

function main(): void {
  assert(sharedTail === "hared", "a module `let` narrowed before the call");
  assert(counter === 2, "an assignment inside flows out");
  assert(readsNarrowedLet() === "bc", "a local `let` narrowed before the call");
  assert(writeFlowsOut() === 3, "the body's write holds after the call");
  assert(writeReplacesNarrowing() === "null", "a write replaces the outer narrowing");
  assert(earlyReturnKeepsDeclaredType(true) === "null", "an early return");
  assert(earlyReturnKeepsDeclaredType(false) === "6", "and its end");
  assert(fieldNarrowing({ label: "four" }) === 4 && fieldNarrowing({ label: null }) === -1, "a field");
  assert(writesInLoop() === 3, "writes in a loop");
}
