// `typeof x === "T"` folds to a constant when the operand's type decides the
// tag, but the operand still has to run: JS evaluates it whatever the answer.
// Folding it away deleted the call outright — a silent wrong answer, since the
// program compiled and produced a result with the side effect missing.
let n: number = 0;

function num(): number {
  n = n + 1;
  return 1;
}

function str(): string {
  n = n + 10;
  return "a";
}

function nothing(): void {
  n = n + 100;
}

function dynamic(): unknown {
  n = n + 1000;
  return 1;
}

function main(): void {
  // Statically true.
  n = 0;
  const t = typeof num() === "number";
  assert(t, "the tag still folds to true");
  assert(n === 1, "the operand ran");

  // Statically false.
  n = 0;
  const f = typeof str() === "number";
  assert(!f, "the tag still folds to false");
  assert(n === 10, "the operand ran");

  // Negated form goes through the same fold.
  n = 0;
  const neg = typeof num() !== "number";
  assert(!neg, "negation of a folded true");
  assert(n === 1, "the operand ran under `!==`");

  // A `void` operand leaves nothing on the stack to discard.
  n = 0;
  const vd = typeof nothing() === "number";
  assert(!vd, "`void` is never `number`");
  assert(n === 100, "the void call ran");

  // In a guard position the constant still decides the branch.
  n = 0;
  let taken = "";
  if (typeof str() === "number") {
    taken = "then";
  } else {
    taken = "else";
  }
  assert(taken === "else", "the folded constant picks the branch");
  assert(n === 10, "the operand ran before the branch");

  // An `unknown` operand is decided at runtime; it was never at risk.
  n = 0;
  const dyn = typeof dynamic() === "number";
  assert(dyn, "runtime tag test on unknown");
  assert(n === 1000, "the dynamic operand ran");

  // A side-effect-free operand folds flat and still reads correctly.
  n = 0;
  const local = 5;
  assert(typeof local === "number", "pure operand folds to true");
  assert(n === 0, "and adds no evaluation");

  // Nested inside a larger expression, the effect happens exactly once.
  n = 0;
  const both = (typeof num() === "number") && (typeof str() === "string");
  assert(both, "both halves fold true");
  assert(n === 11, "each operand ran exactly once");
}
