// Once every literal of a union has been tested for, nothing is left: the
// final branch sees `never`, even when the last test is against a reference
// already narrowed to a single literal.
function assertNever(value: never): number {
  throw new Error("unexpected " + String(value));
}

function letter(x: "a" | "b"): number {
  if (x === "a") {
    return 1;
  } else if (x === "b") {
    return 2;
  }
  return assertNever(x);
}

function digit(x: 1 | 2): number {
  if (x === 1) {
    return 1;
  }
  if (x === 2) {
    return 2;
  }
  return assertNever(x);
}

function flag(x: boolean): number {
  if (x === true) {
    return 1;
  } else if (x === false) {
    return 2;
  }
  return assertNever(x);
}

function letterOrNull(x: "a" | null): number {
  if (x === null) {
    return 0;
  } else if (x === "a") {
    return 1;
  }
  return assertNever(x);
}

function primitive(x: string | number): number {
  if (typeof x === "string") {
    return 1;
  } else if (typeof x === "number") {
    return 2;
  }
  return assertNever(x);
}

function single(x: "b"): number {
  if (x !== "b") {
    return assertNever(x);
  }
  return 2;
}

function localChain(): string {
  let mode: "a" | "b" = "a";
  if (mode === "a") {
    return "A";
  } else if (mode === "b") {
    return "B";
  }
  return "none";
}

let topMode: "a" | "b" = "a";
let topResult = "none";
if (topMode === "a") {
  topResult = "A";
} else if (topMode === "b") {
  topResult = "B";
}

function main(): void {
  console.log(letter("a"), letter("b"), digit(1), digit(2), flag(true), flag(false));
  console.log(letterOrNull(null), letterOrNull("a"), single("b"), primitive("s"), primitive(0));
  console.log(localChain(), topResult);
}
