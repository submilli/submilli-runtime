// Every loop desugar synthesizes statements that sit beside the body — a trailing
// condition check, a loop-variable binding. Codegen resolves locals by name through
// a scope stack, so a body declaration that lands in the *same* block as one of
// those shadows what it reads.
//
// For `do`/`while` that means the condition: it is typed after the body's scope is
// popped, so it reads the outer binding, and the body must keep its own block.

function shadowedBool(): string {
  let go = true;
  let out = "";
  let n = 0;
  do {
    const go = false;
    n = n + 1;
    out = out + (go ? "T" : "F");
  } while (n < 3 && go);
  return out + "|" + n.toString();
}

// The shadow has a different type than the outer binding, so a leak is an
// invalid module rather than a wrong answer.
function shadowedType(): string {
  let go = 5;
  let out = "";
  let n = 0;
  do {
    const go = "inner";
    n = n + 1;
    out = out + go;
  } while (n < 3 && go > 3);
  return out + "|" + n.toString();
}

function shadowedLet(): number {
  let limit = 3;
  let n = 0;
  do {
    let limit = 0;
    n = n + 1 + limit;
  } while (n < limit);
  return n;
}

// The `for-of` lowering splices the body in beside its synthesized loop-variable
// binding, so the body may shadow the loop variable itself.
function shadowLoopVar(xs: Array<number>): number {
  let total = 0;
  for (const v of xs) {
    const v = 9;
    total = total + v;
  }
  return total;
}

function main(): void {
  assert(shadowedBool() === "FFF|3", "condition reads the outer `go`");
  assert(shadowedType() === "innerinnerinner|3", "typed shadow does not leak");
  assert(shadowedLet() === 3, "outer `limit` bounds the loop");
  assert(shadowLoopVar([1, 2]) === 18, "body shadows the for-of loop variable");
}
