// A call can move a narrowed module variable outside the narrowed type, as in
// TypeScript, so an exhaustive `switch` on it may match no case. JavaScript
// then runs off the end and returns `undefined`, which a `number` cannot hold;
// the function throws a catchable `TypeError` instead of trapping.
let tag: "a" | "b" | "c" = "a";

function setC(): void {
  tag = "c";
}

function code(): number {
  if (tag === "c") {
    return 0;
  }
  setC();
  switch (tag) {
    case "a":
      return 1;
    case "b":
      return 2;
  }
}

class Coder {
  code(): number {
    if (tag === "c") {
      return 0;
    }
    setC();
    switch (tag) {
      case "a":
        return 1;
      case "b":
        return 2;
    }
  }
}

function throwsTypeError(run: () => number): boolean {
  try {
    run();
  } catch (error) {
    return error instanceof TypeError;
  }
  return false;
}

function main(): void {
  assert(throwsTypeError(code), "a function falling off its end throws");
  tag = "a";
  const coder = new Coder();
  assert(throwsTypeError(() => coder.code()), "a method falling off its end throws");
  assert(code() === 0, "the guard sees the value the earlier call left");
  console.log("ok");
}
