// One `let`/`const` statement may declare several variables. Each declarator is
// its own declaration, in source order, so a later one reads an earlier one.
let moduleA = 1, moduleB = moduleA + 1;

function main(): void {
  let a = 5, b = 7;
  assert(a + b === 12, "both `let` declarators bind");
  a = 6;
  b = 8;
  assert(a + b === 14, "both stay writable");

  const x = 1, y = "s", z = x + 1;
  assert(z === 2 && y === "s", "a later declarator reads an earlier one");

  let n: number = 0, s = "t";
  n = n + 1;
  assert(n === 1 && s === "t", "annotations are per declarator");

  let empty: number | undefined, after = 3;
  assert(empty === undefined && after === 3, "a declarator without a value starts `undefined`");

  const [first, second] = [1, 2], third = 3;
  const { left, right } = { left: "l", right: "r" }, both = left + right;
  assert(first + second + third === 6, "an array pattern declarator mixes with a plain one");
  assert(both === "lr", "an object pattern declarator mixes with a plain one");

  const closures: Array<() => number> = [];
  {
    let inner = 10, outer = inner * 2;
    closures.push(() => inner + outer);
  }
  assert(closures[0]() === 30, "declarators in a nested block share its scope");

  switch (a) {
    case 6:
      let p = 1, q = 2;
      assert(p + q === 3, "declarators in a `switch` case bind");
      break;
  }

  assert(moduleA === 1 && moduleB === 2, "module-level declarators bind");
}
