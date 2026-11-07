type A = { kind: "a"; x: number };
type Wide = { kind: "wide"; value: number };
type Narrow = { kind: "narrow"; value: 1 };
function make<F extends () => A>(read: F): A { return read(); }
function keep<F extends () => A>(read: F): F { return read; }
function keepChoice<F extends () => Wide | Narrow>(read: F): F { return read; }
function keepEither<F extends (() => Wide) | (() => Narrow)>(read: F): F { return read; }

function main(): void {
  assert(make(() => ({ kind: "a", x: 1 })).kind === "a", "expression callback");
  assert(make(() => { return { kind: "a", x: 2 }; }).x === 2, "block callback");
  assert(make(function () { return { kind: "a", x: 3 }; }).x === 3, "function expression");
  const read = keep(() => {
    function inner(): { kind: string; x: number } { return { kind: "b", x: 4 }; }
    const value = inner();
    value.kind = "c";
    return { kind: "a", x: value.x, extra: value.kind };
  });
  assert(read().extra === "c", "nested function returns have their own context");
  const kind = "wide";
  const choice = keepChoice(() => {
    const kind = "narrow";
    if (kind === "narrow") { return { kind, value: 1, extra: "kept" }; }
    throw new Error("unreachable");
  })();
  const exact: 1 = choice.value;
  assert(exact === 1 && choice.extra === "kept" && kind === "wide", "return context sees local constants and shadowing");
  const wideExpression = keepEither(() => ({ kind: "wide", value: 1 }))();
  wideExpression.value = 2;
  const wideBlock = keepEither(() => { return { kind: "wide", value: 1 }; })();
  wideBlock.value = 2;
  const narrow = keepEither(() => { return { kind: "narrow", value: 1 }; })();
  const one: 1 = narrow.value;
  assert(wideExpression.value === 2 && wideBlock.value === 2 && one === 1, "callable union retains every return context");
}
