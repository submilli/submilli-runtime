// expect-error: function `nothing` returns `unknown` but has no `return`
// expect-error: function `inner` returns `unknown` but has no `return`
// expect-error: function `Box.make` returns `unknown` but has no `return`
// expect-error: method `Box.peek` returns `unknown` but has no `return`
// expect-error: getter `Box.label` has no `return`; a getter must return a value
// expect-error: arrow function returns `unknown` but has no `return`
// expect-error: add a `return` with a value, or a bare `return;`, which yields `undefined`
// expect-error-count: 6
// `unknown` lets a body that returns somewhere fall off the end, but one with
// no `return` of its own is a mistake, as in TypeScript; a `return` in a
// nested closure does not count.
function nothing(): unknown {
  const inner = (): number => {
    return 1;
  };
  inner();
}

class Box {
  static make(): unknown {
    console.log("make");
  }

  peek(): unknown {
    console.log("peek");
  }

  get label(): unknown {
    console.log("label");
  }
}

function main(): void {
  function inner(): unknown {
    console.log("inner");
  }
  const log = (): unknown => {
    console.log("log");
  };
  log();
  inner();
  nothing();
  Box.make();
  new Box().peek();
  console.log(new Box().label === undefined);
}
