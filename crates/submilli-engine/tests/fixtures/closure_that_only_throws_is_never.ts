// An arrow or function expression with no `return` whose end can't be reached
// returns `never`, as tsc infers, so its call fits any result type. A loop
// whose condition is literally `true` and that never breaks doesn't fall
// through either, for closures and declared functions alike.
const fail = () => {
  throw new Error("arrow");
};

const failExpression = function () {
  throw new Error("function expression");
};

const spin = () => {
  while (true) {
    throw new Error("while (true)");
  }
};

function sign(n: number): string {
  if (n > 0) {
    return "positive";
  }
  return fail();
}

function firstPass(): number {
  for (;;) {
    throw new Error("for (;;)");
  }
}

function retryOnce(): number {
  do {
    throw new Error("do-while");
  } while (Math.random() > 2);
}

function countTo(limit: number): number {
  let i = 0;
  while (true) {
    i++;
    if (i >= limit) {
      break;
    }
  }
  return i;
}

function pick(n: number): number {
  for (;;) {
    switch (n) {
      case 1:
        break;
      default:
        return n;
    }
  }
}

function report(run: () => unknown): void {
  try {
    run();
  } catch (e) {
    console.log((e as Error).message);
  }
}

function main(): void {
  console.log(sign(1));
  const asNumber: () => number = fail;
  report(asNumber);
  report(failExpression);
  report(spin);
  report(firstPass);
  report(retryOnce);
  console.log(countTo(3), pick(2));
}
