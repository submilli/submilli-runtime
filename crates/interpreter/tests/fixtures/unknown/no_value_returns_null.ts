// A body returning `unknown` that ends without a `return`, or returns with a
// bare `return;`, yields `undefined`.
type Anything = unknown;

function pick(flag: boolean): unknown {
  if (flag) {
    return 1;
  }
}

function aliased(flag: boolean): Anything {
  if (flag) {
    return "x";
  }
}

function early(flag: boolean): unknown {
  if (flag) {
    return;
  }
  return 4;
}

interface Reader {
  read(flag: boolean): unknown;
}

function viaLoop(limit: number): unknown {
  for (let i = 0; i < limit; i++) {
    if (i === 2) {
      return;
    }
  }
  return "done";
}

function viaFinally(flag: boolean): unknown {
  try {
    if (flag) {
      return 9;
    }
  } finally {
    console.log("finally");
  }
}

class Base {
  private n: number = 0;

  static make(flag: boolean): unknown {
    if (flag) {
      return;
    }
    return "made";
  }

  get bare(): unknown {
    return;
  }

  read(flag: boolean): unknown {
    if (flag) {
      return true;
    }
  }

  skip(flag: boolean): unknown {
    if (flag) {
      return;
    }
    return "kept";
  }

  get value(): unknown {
    if (this.n > 0) {
      return this.n;
    }
  }
}

class Derived extends Base {
  read(flag: boolean): unknown {
    if (!flag) {
      return 7;
    }
  }
}

function main(): void {
  assert(pick(false) === undefined);
  assert(pick(true) === 1);
  assert(aliased(false) === undefined);
  assert(aliased(true) === "x");
  assert(early(true) === undefined);
  assert(early(false) === 4);
  assert(viaLoop(5) === undefined);
  assert(viaLoop(1) === "done");
  assert(viaFinally(false) === undefined);
  assert(viaFinally(true) === 9);
  assert(Base.make(true) === undefined);
  assert(Base.make(false) === "made");

  const base = new Base();
  assert(base.read(false) === undefined);
  assert(base.read(true) === true);
  assert(base.value === undefined);
  assert(base.skip(true) === undefined);
  assert(base.skip(false) === "kept");
  assert(base.bare === undefined);
  const reader: Reader = new Derived();
  assert(reader.read(true) === undefined);

  const derived: Base = new Derived();
  assert(derived.read(true) === undefined);
  assert(derived.read(false) === 7);

  const arrow = (flag: boolean): unknown => {
    if (flag) {
      return 2;
    }
  };
  assert(arrow(false) === undefined);
  assert(arrow(true) === 2);
  const annotatedBare = (flag: boolean): unknown => {
    if (flag) {
      return;
    }
    return 5;
  };
  assert(annotatedBare(true) === undefined);
  assert(annotatedBare(false) === 5);

  // A closure typed `unknown` only by its context behaves as an annotated
  // one: its returns need not agree, and it may fall off the end.
  const contextual: () => unknown = () => {
    return;
  };
  assert(contextual() === undefined);
  const mixed: (flag: boolean) => unknown = (flag) => {
    if (flag) {
      return null;
    }
    return 2;
  };
  assert(mixed(true) === null);
  assert(mixed(false) === 2);
  const partial: (flag: boolean) => unknown = (flag) => {
    if (flag) {
      return "p";
    }
  };
  assert(partial(false) === undefined);
  assert(partial(true) === "p");

  const mapped = [1, 2].map((x): unknown => {
    if (x > 1) {
      return x;
    }
  });
  assert(mapped[0] === undefined);
  assert(mapped[1] === 2);

  const literal = {
    get(flag: boolean): unknown {
      if (flag) {
        return;
      }
      return "lit";
    },
  };
  assert(literal.get(true) === undefined);
  assert(literal.get(false) === "lit");

  function nested(flag: boolean): unknown {
    if (flag) {
      return 3;
    }
    if (!flag) {
      return;
    }
  }
  assert(nested(false) === undefined);
  assert(nested(true) === 3);
}
