// A function declared in a block at the top level of a module is a closure over
// that block's locals, as one in a function body is. So is an arrow there.

let escaped: () => number = (): number => -1;
const fromLoop: (() => number)[] = [];
let fromCase = "";
let fromCatch = "";

{
  assert(early() === 42, "hoisted to the start of its block");
  function early(): number {
    return 42;
  }

  const base = 10;
  function fact(n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  }
  function isEven(n: number): boolean {
    return n === 0 ? true : isOdd(n - 1);
  }
  function isOdd(n: number): boolean {
    return n === 0 ? false : isEven(n - 1);
  }
  function addBase(n: number): number {
    return n + base;
  }
  assert(fact(5) === 120, "calls itself");
  assert(isEven(10) && isOdd(7), "calls its siblings");
  assert(addBase(1) === 11, "reads a block local");

  let shared = 1;
  function readShared(): number {
    return shared;
  }
  shared = 5;
  escaped = readShared;
}
assert(escaped() === 5, "outlives its block and sees the last write");

{
  const limit = 3;
  function viaSibling(): number {
    return capturesLimit();
  }
  function capturesLimit(): number {
    return limit;
  }
  assert(viaSibling() === 3, "called through a hoisted sibling");
}

{
  let counter = 0;
  function bump(): number {
    counter = counter + 1;
    return counter;
  }
  bump();
  bump();
  assert(counter === 2, "writes a block local");
  assert(bump() === 3, "keeps writing the same binding");
}

{
  let maybe: string | null = "set";
  function clear(): void {
    maybe = null;
  }
  if (maybe !== null) {
    clear();
    assert(maybe === null, "a local a block function writes isn't narrowed");
  }
}

{
  const outer = 4;
  {
    function readsOuter(): number {
      return outer;
    }
    assert(readsOuter() === 4, "reads an enclosing block's local");
  }
}

for (let i = 0; i < 3; i++) {
  function captured(): number {
    return i;
  }
  fromLoop.push(captured);
}
assert(fromLoop[0]() === 0 && fromLoop[2]() === 2, "one binding per iteration");

let words = "";
for (const word of ["a", "b"]) {
  function echo(): string {
    return word;
  }
  words = words + echo();
}
assert(words === "ab", "in a `for…of` body");

let rounds = 0;
while (rounds < 2) {
  const label = "round " + String(rounds);
  function describe(): string {
    return label;
  }
  assert(describe() === "round " + String(rounds), "in a `while` body");
  rounds = rounds + 1;
}

switch (1 as number) {
  case 1: {
    const k = "case";
    function inCase(): string {
      return k;
    }
    fromCase = inCase();
    break;
  }
}
assert(fromCase === "case", "in a `case` block");

try {
  throw new Error("boom");
} catch (e) {
  function message(): string {
    return e.message;
  }
  fromCatch = message();
}
assert(fromCatch === "boom", "captures the `catch` binding");

{
  const seven = 7;
  const arrow = (): number => seven;
  const viaNested = (): number => {
    function inner(): number {
      return seven;
    }
    return inner();
  };
  assert(arrow() === 7, "an arrow captures a block local");
  assert(viaNested() === 7, "so does a function nested in that arrow");
}

function main(): void {}
