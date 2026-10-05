// A closure may read a local that a guard narrowed to `never`; the code
// holding it never runs, so creating the closure doesn't need the value.
function viaArrow(x: string | number): number {
  if (typeof x === "string") {
    return 1;
  }
  if (typeof x === "number") {
    return 2;
  }
  const inner = (): number => (x ? 3 : 4);
  return inner();
}

function viaFunctionExpression(x: "a" | "b"): string {
  if (x === "a") {
    return "A";
  } else if (x === "b") {
    return "B";
  }
  return (function (): string {
    return String(x);
  })();
}

function viaContradiction(x: number | null): string {
  if (x !== null) {
    if (x === null) {
      const read = (): string => String(x);
      return read();
    }
    return "number";
  }
  return "null";
}

// A `never` element holds no value, so it doesn't fix the array's type.
function inArray(x: string | number): number {
  if (typeof x === "string") {
    return 1;
  }
  if (typeof x === "number") {
    return 2;
  }
  const values = [x, 3];
  return values.length;
}

// The right side rewrites `a` after the left side read it, so the comparison
// says nothing about the value `a` holds afterwards.
function rewrittenByOtherSide(start: "x" | "y"): string {
  let a: "x" | "y" = start;
  if (
    a ===
    ((): "y" => {
      a = "y";
      return "y";
    })()
  ) {
    return "same";
  }
  if (a === "x") {
    return "x";
  }
  return "now " + a;
}

function main(): void {
  console.log(viaArrow("s"), viaArrow(1), viaFunctionExpression("a"), viaFunctionExpression("b"));
  console.log(viaContradiction(1), viaContradiction(null));
  console.log(inArray("s"), inArray(1), rewrittenByOtherSide("x"), rewrittenByOtherSide("y"));
}
