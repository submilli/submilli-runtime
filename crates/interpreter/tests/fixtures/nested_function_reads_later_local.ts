// A nested function may read and write locals of its block declared below it. Its
// closure is created once the last of those is declared, so calls after that see
// them, as in JavaScript.

function readsLaterLocals(): string {
  function scaled(): number {
    return factor * 10 + offset();
  }
  function offset(): number {
    return step;
  }
  let factor = 1;
  const step = 2;
  const first = scaled();
  factor = 5;
  return String(first) + "," + String(scaled());
}
assert(readsLaterLocals() === "12,52", "reads later locals and sees later writes");

function mutualRecursion(): string {
  function isEven(n: number): boolean {
    return n === 0 ? base : isOdd(n - 1);
  }
  function isOdd(n: number): boolean {
    return n === 0 ? !base : isEven(n - 1);
  }
  const base = true;
  return String(isEven(4)) + String(isOdd(3)) + String(isEven(3));
}
assert(mutualRecursion() === "truetruefalse", "siblings recurse through a later local");

function perIteration(): string {
  let out = "";
  for (const k of [1, 2]) {
    function show(): string {
      return label + String(k);
    }
    const label = "k=";
    out += show();
  }
  return out;
}
assert(perIteration() === "k=1k=2", "a later local in a loop body");

function writesThroughLaterArrow(): number {
  function next(): number {
    return increment();
  }
  let count = 0;
  const increment = (): number => {
    count += 1;
    return count;
  };
  next();
  next();
  return next();
}
assert(writesThroughLaterArrow() === 3, "calls an arrow declared below it");

function usedAsValueAfterward(): string {
  function greet(): string {
    return word;
  }
  const word = "hi";
  const alias = greet;
  const all = [greet, alias];
  return all[0]() + all[1]();
}
assert(usedAsValueAfterward() === "hihi", "taken as a value once its locals exist");

function arrowInsideNested(): number {
  function outer(): number {
    const inner = (): number => answer + 1;
    return inner();
  }
  const answer = 41;
  return outer();
}
assert(arrowInsideNested() === 42, "an arrow inside it reads the later local too");

function destructuredLocals(): string {
  function describe(): string {
    return String(first) + String(second) + name + String(rest.a + rest.b);
  }
  const [first, second] = [1, 2];
  const { name, ...rest } = { name: "n", a: 1, b: 2 };
  return describe();
}
assert(destructuredLocals() === "12n3", "destructured and rest locals");

function nestedTwice(): number {
  function outer(): number {
    function inner(): number {
      return depth;
    }
    return inner() + 1;
  }
  const depth = 5;
  return outer();
}
assert(nestedTwice() === 6, "a function nested in it reads the later local");

function inCatchAndCase(kind: number): string {
  let out = "";
  try {
    throw new Error("boom");
  } catch (e) {
    function caught(): string {
      return suffix + (e instanceof Error ? e.message : "");
    }
    const suffix = "caught ";
    out += caught();
  }
  switch (kind) {
    case 1: {
      function inCase(): string {
        return label;
      }
      const label = " case";
      out += inCase();
      break;
    }
  }
  return out;
}
assert(inCatchAndCase(1) === "caught boom case", "in a `catch` block and a `case` block");

{
  function topBlock(): number {
    return blockLocal * 2;
  }
  const blockLocal = 21;
  assert(topBlock() === 42, "in a block at the top level of a module");
}

function main(): void {}
