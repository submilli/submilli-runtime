// Nested function declarations in each kind of enclosing body, and capturing
// each kind of local.

function helper(): string {
  return "outer";
}

function shadows(): string {
  let r = helper();
  {
    function helper(): string {
      return "inner";
    }
    r += helper();
  }
  return r + helper();
}

function counter(): () => number {
  let n = 0;
  function next(): number {
    n++;
    return n;
  }
  return next;
}

function firstOf<T>(xs: T[], fallback: T): T {
  function pick(): T {
    return xs.length > 0 ? xs[0] : fallback;
  }
  return pick();
}

class Shapes {
  side: number;
  constructor(side: number) {
    function double(x: number): number {
      return x * 2;
    }
    this.side = double(side);
  }
  get area(): number {
    const s = this.side;
    function square(): number {
      return s * s;
    }
    return square();
  }
  static unit(): number {
    function one(): number {
      return 1;
    }
    return one();
  }
}

function captures(): string {
  const { a, b } = { a: 1, b: 2 };
  let out = "";
  for (const v of [10, 20]) {
    function show(): string {
      return String(v + a + b);
    }
    out += show();
  }
  try {
    throw new Error("boom");
  } catch (e) {
    function describe(): string {
      return e instanceof Error ? e.message : "?";
    }
    out += describe();
  }
  return out;
}

function inCallbacks(): string {
  const fromArrow = (k: number): number => {
    function inc(x: number): number {
      return x + 1;
    }
    return inc(k);
  };
  const obj = {
    run(): string {
      function tag(): string {
        return "obj";
      }
      return tag();
    },
  };
  return String(fromArrow(1)) + obj.run();
}

// A nested function or a block-bodied arrow that returns does not end the
// enclosing function's reachable code, so narrowing after an early return holds.
function afterClosures(v: string | number): string {
  function one(): number {
    return 1;
  }
  const two = (): number => {
    return 2;
  };
  if (typeof v === "string") {
    return v;
  }
  return String(v + one() + two());
}

// A nested function in a loop body, with the enclosing variable reassigned to
// another member of its union.
function loopReassign(): string {
  let cur: string | number = 1;
  for (let n = 0; n < 2; n++) {
    function seven(): number {
      return 7;
    }
    cur = "s" + String(seven());
  }
  return String(cur);
}

function sumTo(n: number): number {
  function go(k: number, acc: number): number {
    return k === 0 ? acc : go(k - 1, acc + k);
  }
  return go(n, 0);
}

function main(): void {
  assert(shadows() === "outerinnerouter", "shadows a top-level function");
  const next = counter();
  next();
  assert(next() === 2, "returned and keeps its state");
  assert(firstOf([4, 5], 0) === 4 && firstOf<string>([], "z") === "z", "uses the enclosing type parameter");
  const shape = new Shapes(3);
  assert(shape.side === 6 && shape.area === 36 && Shapes.unit() === 1, "constructor, getter, static");
  assert(captures() === "1323boom", "destructured, for-of and catch bindings");
  assert(inCallbacks() === "2obj", "arrow body and object method");
  assert(afterClosures("a") === "a" && afterClosures(1) === "4", "narrowing after an early return");
  assert(loopReassign() === "s7", "loop with a reassigned union");
  assert(sumTo(1000) === 500500, "deep recursion");
}
