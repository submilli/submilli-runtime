// The `for` update clause runs on the back edge — after the body, with the
// condition having held — not on the loop-exit environment. Typing it under the
// exit environment reported `cur`'s type as `null` inside `cur = cur.next`,
// which is the idiomatic linked-list walk.

interface Node {
  value: number;
  next: Node | null;
}

function walk(head: Node | null): number {
  let sum = 0;
  for (let cur = head; cur !== null; cur = cur.next) {
    sum = sum + cur.value;
  }
  return sum;
}

// The body's own writes reach the update clause: `cur` is reassigned to a
// non-null node, and the update still reads it narrowed.
function reassignInBody(head: Node): number {
  let steps = 0;
  for (let cur: Node | null = head; cur !== null; cur = cur.next) {
    cur = cur.next === null ? cur : cur.next;
    steps = steps + 1;
  }
  return steps;
}

// A `continue` is a back edge too, so its state joins the natural body end
// before the update clause is typed.
function continueJoinsBackEdge(head: Node): number {
  let steps = 0;
  for (let cur: Node | null = head; cur !== null; cur = cur.next) {
    if (cur.value < 0) {
      continue;
    }
    steps = steps + 1;
  }
  return steps;
}

// The update clause's own narrowing must not escape the loop: after the loop
// the condition has failed, so `cur` is null there.
function exitStateIsStillFalseBranch(head: Node): string {
  let cur: Node | null = head;
  for (; cur !== null; cur = cur.next) {
    // walk to the end
  }
  return cur === null ? "ended" : "unreachable";
}

// No init clause: the cursor is declared outside the loop.
function noInit(head: Node | null): number {
  let sum = 0;
  let cur = head;
  for (; cur !== null; cur = cur.next) {
    sum = sum + cur.value;
  }
  return sum;
}

// The arithmetic update forms reach the update slot through different lowerings
// (`PostfixUnary` and `CompoundAssign` are rewritten after the loop desugar).
function counters(): string {
  let a = 0;
  for (let i = 0; i < 5; i++) {
    a = a + i;
  }
  let b = 0;
  for (let i = 5; i > 0; i--) {
    b = b + 1;
  }
  let c = 0;
  for (let i = 0; i < 6; i += 2) {
    c = c + 1;
  }
  return `${a}|${b}|${c}`;
}

function withBreak(head: Node): number {
  let sum = 0;
  for (let cur: Node | null = head; cur !== null; cur = cur.next) {
    if (cur.value === 3) {
      break;
    }
    sum = sum + cur.value;
  }
  return sum;
}

// A write in the update clause invalidates a guard from *outside* the loop:
// every exit runs the update, so the pre-loop narrowing is dead after it.
function updateInvalidatesOuterGuard(head: Node): string {
  let cur: Node | null = head;
  if (cur !== null) {
    for (let i = 0; i < 2; cur = null) {
      i = i + 1;
    }
    return cur === null ? "cleared" : "kept";
  }
  return "none";
}

function main(): void {
  const list: Node = { value: 1, next: { value: 2, next: { value: 3, next: null } } };
  assert(walk(list) === 6, "linked-list walk with the update clause");
  assert(walk(null) === 0, "empty list never runs the body or the update");
  assert(reassignInBody(list) === 2, "update reads what the body left behind");
  assert(continueJoinsBackEdge(list) === 3, "continue reaches the update clause");
  assert(exitStateIsStillFalseBranch(list) === "ended", "loop exit keeps the false branch");
  assert(noInit(list) === 6, "no init clause");
  assert(noInit(null) === 0, "no init clause, empty list");
  assert(counters() === "10|5|3", "postfix, prefix-style, and compound updates");
  assert(withBreak({ value: 3, next: null }) === 0, "break before the first add");
  assert(withBreak(list) === 3, "break at the third node, after 1 + 2");
  assert(updateInvalidatesOuterGuard(list) === "cleared", "update clears an outer guard");
}
