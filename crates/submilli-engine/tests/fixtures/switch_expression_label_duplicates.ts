// An expression label is never a duplicate, whichever comes first, as in tsc:
// the first matching arm wins at run time.
function exprFirst(x: number): string {
  const one: 1 = 1;
  switch (x) {
    case one:
      return "expr";
    case 1:
      return "lit";
    default:
      return "other";
  }
}

function literalFirst(x: number): string {
  const one: 1 = 1;
  switch (x) {
    case 1:
      return "lit";
    case one:
      return "expr";
    default:
      return "other";
  }
}

function main(): void {
  assert(exprFirst(1) === "expr", "the expression label matches first");
  assert(literalFirst(1) === "lit", "the literal label matches first");
}
