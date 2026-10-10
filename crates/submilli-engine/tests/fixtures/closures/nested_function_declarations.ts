// Function declarations inside a function body are local closures, hoisted to
// the start of their block as in JavaScript.

function factorial(n: number): number {
  function go(k: number): number {
    return k <= 1 ? 1 : k * go(k - 1);
  }
  return go(n);
}

function parity(n: number): string {
  function isEven(k: number): boolean {
    return k === 0 ? true : isOdd(k - 1);
  }
  function isOdd(k: number): boolean {
    return k === 0 ? false : isEven(k - 1);
  }
  return isEven(n) ? "even" : "odd";
}

// Called above its declaration: it captures no local of its block.
function total(values: number[]): number {
  const sum = add(values);
  return sum * 2;
  function add(xs: number[]): number {
    let t = 0;
    for (const x of xs) {
      t += x;
    }
    return t;
  }
}

// It captures `base`, so it exists from `base`'s declaration on, and a `let`
// it writes stays shared with the enclosing function.
function capturing(): number[] {
  const base = 10;
  let calls = 0;
  const first = shift(1);
  function shift(x: number): number {
    calls++;
    return x + base;
  }
  return [first, shift(2), calls];
}

// A sibling it calls captures a local: both exist once that local does.
function throughSibling(): number {
  const offset = 5;
  function outer(): number {
    return inner() + 1;
  }
  const r = outer();
  function inner(): number {
    return offset;
  }
  return r;
}

function inBlocks(flag: boolean): string {
  if (flag) {
    function yes(): string {
      return "yes";
    }
    return yes();
  }
  let last = "";
  for (let i = 0; i < 2; i++) {
    function label(k: number): string {
      return "i" + String(k);
    }
    last = label(i);
  }
  switch (last) {
    case "i1": {
      function hit(): string {
        return "hit";
      }
      return hit();
    }
  }
  return last;
}

// While and do-while bodies declare theirs per pass too; a function is hoisted
// above its declaration within the body.
function whileBodies(): string {
  let out = "";
  let i = 0;
  while (i < 2) {
    out += early();
    function early(): string {
      return "w";
    }
    i++;
  }
  do {
    const k = i;
    function show(): string {
      return String(k);
    }
    out += show();
    i++;
  } while (i < 4);
  return out;
}

// Each pass through the loop body declares a fresh function over its own `i`.
function perIteration(): number[] {
  const fns: (() => number)[] = [];
  for (let i = 0; i < 3; i++) {
    function get(): number {
      return i * 10;
    }
    fns.push(get);
  }
  return fns.map((f) => f());
}

function guards(values: (string | number)[]): number {
  function isText(v: string | number): v is string {
    return typeof v === "string";
  }
  let n = 0;
  for (const v of values) {
    if (isText(v)) {
      n += v.length;
    }
  }
  return n;
}

function asValues(): string {
  function greet(name: string): string {
    return "hi " + name;
  }
  const alias = greet;
  return ["a", "b"].map(greet).join(",") + "|" + alias("c");
}

function rest(): string {
  function join(...parts: string[]): string {
    return parts.join("-");
  }
  return join("a", "b", "c") + "|" + join();
}

function deep(n: number): number {
  function outer(a: number): number {
    function inner(b: number): number {
      return b + n;
    }
    return inner(a) * 2;
  }
  return outer(1);
}

class Counter {
  count: number = 0;
  addTwice(by: number): number {
    function twice(x: number): number {
      return x * 2;
    }
    this.count += twice(by);
    return this.count;
  }
}

function main(): void {
  assert(factorial(5) === 120, "self-recursion");
  assert(parity(7) === "odd" && parity(4) === "even", "mutual recursion");
  assert(total([1, 2, 3]) === 12, "called before its declaration");
  const c = capturing();
  assert(c[0] === 11 && c[1] === 12 && c[2] === 2, "captures a local and a let");
  assert(throughSibling() === 6, "through a capturing sibling");
  assert(inBlocks(true) === "yes", "in an if block");
  assert(inBlocks(false) === "hit", "in loop and switch blocks");
  const each = perIteration();
  assert(each[0] === 0 && each[1] === 10 && each[2] === 20, "one closure per iteration");
  assert(guards(["ab", 3, "cde"]) === 5, "type guard");
  assert(asValues() === "hi a,hi b|hi c", "as a value");
  assert(rest() === "a-b-c|", "rest parameter");
  assert(deep(5) === 12, "nested twice");
  assert(new Counter().addTwice(3) === 6, "inside a method");
  assert(whileBodies() === "ww23", "while and do-while bodies");
}
