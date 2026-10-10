// A closure's wasm return slot is the erased (ref null $Object), so a
// primitive result has to box even when the declared return type is already
// reference-lowered — `number | null`, an alias for it, or `unknown`. The other
// three slot kinds are here too: the slot a method gets through a vtable, the
// re-derived slot of a top-level function, and the absent slot of a void
// closure, aliased or not.
//
// Stays a top-level fixture: the smoke suite runs every top-level file but only
// the alphabetically first one per subdirectory, so moving this under closures/
// would silently drop it from the default run.

type MaybeNumber = number | null;
type Nothing = void;

class Cell {
  constructor(readonly v: number) {}
}

class Source {
  find(n: number): number | null {
    return n > 0 ? n : null;
  }
}

class Doubling extends Source {
  find(n: number): number | null {
    return n > 0 ? n * 2 : null;
  }
}

function apply(f: (n: number) => number | null, n: number): number | null {
  return f(n);
}

function identity<T>(v: T): T {
  return v;
}

function boom(): never {
  throw new Error("boom");
}

function main(): void {
  const expr = (x: number): number | null => x + 2;
  assert(expr(1) === 3, "expression body boxes the number");

  const block = (x: number): number | null => {
    return x + 2;
  };
  assert(block(1) === 3, "block body boxes the number");

  assert(apply(expr, 4) === 6, "boxed result survives an indirect call");

  const aliased = (x: number): MaybeNumber => x + 2;
  assert(aliased(1) === 3, "alias for the union boxes the same way");

  const bool = (x: number): boolean | null => x > 0;
  assert(bool(1) === true, "boolean boxes into the nullable slot");
  assert(bool(-1) === false, "false is not null");

  const unknownReturn = (x: number): unknown => x + 1;
  assert(unknownReturn(1) !== null, "unknown return boxes too");

  const mixed = (x: number): number | null => (x > 0 ? x : null);
  assert(mixed(2) === 2, "union-typed value needs no extra boxing");
  assert(mixed(-2) === null, "null branch stays null");

  const str = (x: number): string | null => (x > 0 ? "pos" : null);
  assert(str(1) === "pos", "reference result passes through");

  const cell = (x: number): Cell | null => new Cell(x);
  const got = cell(3);
  assert(got !== null && got.v === 3, "class instance passes through");

  // The value's own type drives the coercion, so a generic call's erased result
  // lands in the slot correctly too.
  const viaGeneric = (x: number): number | null => identity(x);
  assert(viaGeneric(4) === 4, "generic call result reaches the slot");

  const nested = (x: number): number | null => {
    const inner = (y: number): number | null => y * 2;
    const r = inner(x);
    return r === null ? null : r + 1;
  };
  assert(nested(2) === 5, "nested closures each box their own result");

  // A method's slot is recorded by the vtable, not erased like a closure's.
  const src: Source = new Doubling();
  assert(src.find(3) === 6, "nullable primitive through a vtable slot");
  assert(src.find(-1) === null, "null through a vtable slot");

  // `finally` stashes the return value in a local typed from the erased slot,
  // so the value has to be boxed before it reaches the stash.
  let ran = false;
  const stashed = (x: number): number | null => {
    try {
      return x + 1;
    } finally {
      ran = true;
    }
  };
  assert(stashed(1) === 2, "value boxes before the finally stash");
  assert(ran, "finally still ran");

  // A `never` value under a primitive declared return: nothing reaches the
  // slot, so the coercion must not emit a box against an empty stack.
  const throws = (x: number): number => boom();
  let caught = false;
  try {
    throws(1);
  } catch (e) {
    caught = true;
  }
  assert(caught, "never-typed return traps instead of coercing");

  // A void closure has no result slot, but a diverging body still pushes the
  // callee's result — it has to be dropped or the body falls off the end with
  // a value on the stack.
  let seen = 0;
  const runVoid = (f: (n: number) => void): void => {
    f(1);
  };
  runVoid((n: number): void => {
    seen = seen + n;
  });
  assert(seen === 1, "plain void closure still runs");
  let voidThrew = false;
  try {
    runVoid((n: number): void => boom());
  } catch (e) {
    voidThrew = true;
  }
  assert(voidThrew, "diverging void closure drops its value and traps");

  // Same slot reached through `return`, where the drop is redundant because
  // `return` is stack-polymorphic. These two are guards, not regressions: they
  // pin that the diverging call really does push a value, so the drop can
  // never underflow, and that a finally chain after it stays balanced.
  let blockThrew = false;
  try {
    runVoid((n: number): void => {
      return boom();
    });
  } catch (e) {
    blockThrew = true;
  }
  assert(blockThrew, "diverging void closure traps on the return path");
  let cleanedUp = false;
  let finallyThrew = false;
  try {
    runVoid((n: number): void => {
      try {
        return boom();
      } finally {
        cleanedUp = true;
      }
    });
  } catch (e) {
    finallyThrew = true;
  }
  assert(finallyThrew, "diverging void closure traps through a finally");
  assert(cleanedUp, "the finally still ran on the way out");

  // An aliased `void` return has to reach the same verdict as the funcref
  // signature, which peels — otherwise the closure gets a result slot its
  // signature lacks, and a diverging body fills it.
  let noted = 0;
  const note = (n: number): void => {
    noted = noted + n;
  };
  runVoid((n: number): Nothing => note(n));
  assert(noted === 1, "aliased void closure runs");
  let aliasThrew = false;
  try {
    runVoid((n: number): Nothing => boom());
  } catch (e) {
    aliasThrew = true;
  }
  assert(aliasThrew, "aliased void closure with a diverging body");
}
