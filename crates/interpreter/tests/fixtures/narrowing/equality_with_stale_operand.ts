// A field or global narrowed to a literal can change behind its guard (through
// an alias or in a call), so comparing a local with it narrows nothing: later
// tests that rule the local out must not leave it `never` while it holds a value.
class Box {
  f: "a" | "b" | "c" = "a";
}

function setToB(box: Box): void {
  box.f = "b";
}

function afterCall(box: Box, m: "a" | "b" | "c"): string {
  if (box.f === "a") {
    setToB(box);
    if (m === box.f) {
      if (m === "a") {
        return "a";
      }
      return "m " + m;
    }
  }
  return "end";
}

function afterAliasWrite(x: "a" | "b"): string {
  const o = { y: "a" as "a" | "b" };
  const alias = o;
  o.y = "a";
  alias.y = "b";
  if (x === o.y) {
    return "equal";
  } else if (x === "b") {
    return "B";
  }
  return "kept " + x;
}

let current: "a" | "b" = "a";

function setCurrent(): void {
  current = "b";
}

function afterGlobalWrite(x: "a" | "b"): string {
  if (current === "a") {
    setCurrent();
    if (current === x) {
      return "equal";
    } else if (x === "b") {
      return "B";
    }
    return "fell " + x;
  }
  return "current b";
}

function main(): void {
  console.log(afterCall(new Box(), "b"), afterAliasWrite("a"), afterGlobalWrite("a"));
}
