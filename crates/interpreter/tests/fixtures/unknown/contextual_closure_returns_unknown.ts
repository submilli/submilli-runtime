// A block-bodied closure typed `unknown` only by where it is used takes
// `unknown` as its return type: its returns need not agree, a bare `return;`
// yields `null`, and it may fall off the end.
interface Handler {
  handle: (flag: boolean) => unknown;
}

class Holder {
  readonly run: (flag: boolean) => unknown = (flag) => {
    if (flag) {
      return 1;
    }
  };
}

function call(f: (flag: boolean) => unknown, flag: boolean): unknown {
  return f(flag);
}

function second<T>(first: T, f: (flag: boolean) => T): T {
  return f(first === null);
}

function main(): void {
  assert(call((flag) => {
    if (flag) {
      return;
    }
    return "x";
  }, true) === null);
  assert(call((flag) => {
    if (flag) {
      return;
    }
    return "x";
  }, false) === "x");

  const handler: Handler = {
    handle: (flag) => {
      if (flag) {
        return true;
      }
      return 3;
    },
  };
  assert(handler.handle(true) === true);
  assert(handler.handle(false) === 3);

  const holder = new Holder();
  assert(holder.run(false) === null);
  assert(holder.run(true) === 1);

  const seed: unknown = null;
  const result = second(seed, (flag) => {
    if (flag) {
      return "seeded";
    }
    return 0;
  });
  assert(result === "seeded");
}
