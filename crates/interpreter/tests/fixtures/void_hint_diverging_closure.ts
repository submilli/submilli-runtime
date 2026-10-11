// An unannotated arrow whose body always diverges infers `never`, which
// occupies a value slot. Under a contextual `void` signature — whose funcref
// has no result at all — it has to take the context's verdict instead, or it
// is emitted against a signature the call site cannot name.
//
// The cases below walk the two axes that decide the verdict: which body shapes
// count as diverging, and which positions can carry the `void` hint. The
// annotated spellings are covered by `closure_nullable_return.ts`.

type Nothing = void;

function boom(): never {
  throw new Error("boom");
}

function runVoid(f: (n: number) => void): void {
  f(1);
}

function main(): void {
  let unannotatedThrew = false;
  try {
    runVoid((n: number) => boom());
  } catch (e) {
    unannotatedThrew = true;
  }
  assert(unannotatedThrew, "unannotated diverging closure adopts the void context");
  let unannotatedBlockThrew = false;
  try {
    runVoid((n: number) => {
      boom();
    });
  } catch (e) {
    unannotatedBlockThrew = true;
  }
  assert(unannotatedBlockThrew, "unannotated diverging block body under void");
  // Non-diverging unannotated closures under the same context are unaffected.
  let ranUnannotated = 0;
  runVoid((n: number) => {
    ranUnannotated = ranUnannotated + n;
  });
  assert(ranUnannotated === 1, "unannotated void closure still runs");
  // A non-void context keeps reading the body's type, so a diverging closure
  // there still gets its value slot.
  const applyNumber = (f: (n: number) => number): number => f(1);
  let valueSlotThrew = false;
  try {
    applyNumber((n: number) => boom());
  } catch (e) {
    valueSlotThrew = true;
  }
  assert(valueSlotThrew, "diverging closure under a value-returning context");
  assert(applyNumber((n: number) => n + 1) === 2, "value-returning context unaffected");

  // The same verdict has to hold wherever the `void` hint comes from, and for
  // every body shape that diverges.
  let throwThrew = false;
  try {
    runVoid((n: number) => {
      throw new Error("direct");
    });
  } catch (e) {
    throwThrew = true;
  }
  assert(throwThrew, "a direct throw body diverges the same way");
  // Divergence on only one path types the body `void`, not `never`.
  let partial = 0;
  runVoid((n: number) => {
    if (n < 0) {
      boom();
    }
    partial = n;
  });
  assert(partial === 1, "a conditionally diverging body is void, not never");
  // The hint has to peel a union and an alias to find the `void`.
  const runMaybe = (f: ((n: number) => void) | null): void => {
    if (f !== null) {
      f(1);
    }
  };
  let maybeThrew = false;
  try {
    runMaybe((n: number) => boom());
  } catch (e) {
    maybeThrew = true;
  }
  assert(maybeThrew, "nullable void-fn hint still reaches the void return");
  const runAliased = (f: (n: number) => Nothing): void => {
    f(1);
  };
  let aliasedHintThrew = false;
  try {
    runAliased((n: number) => boom());
  } catch (e) {
    aliasedHintThrew = true;
  }
  assert(aliasedHintThrew, "an aliased void hint is adopted unannotated too");
  // Hint carriers other than a function parameter.
  const slot: ((n: number) => void)[] = [(n: number) => boom()];
  let fromArrayThrew = false;
  try {
    slot[0](1);
  } catch (e) {
    fromArrayThrew = true;
  }
  assert(fromArrayThrew, "an array element type carries the void hint");
  const byMap = new Map<string, (n: number) => void>();
  byMap.set("k", (n: number) => boom());
  let fromMapThrew = false;
  try {
    const f = byMap.get("k");
    if (f !== undefined) {
      f(1);
    }
  } catch (e) {
    fromMapThrew = true;
  }
  assert(fromMapThrew, "a Map value type carries the void hint");
  // Nested: the inner arrow takes its own hint from the inner call.
  let nestedThrew = false;
  try {
    runVoid((n: number) => {
      runVoid((k: number) => boom());
    });
  } catch (e) {
    nestedThrew = true;
  }
  assert(nestedThrew, "a nested diverging arrow adopts its own void hint");
  // The prelude HOFs whose callback slot is `void`.
  let forEachThrew = false;
  try {
    [1, 2].forEach((v: number) => boom());
  } catch (e) {
    forEachThrew = true;
  }
  assert(forEachThrew, "Array#forEach's callback slot is void");
}
