// A `return` from inside a `finally` still runs every *enclosing* `finally` on
// the way out — it continues the chain outward from the block it stands in,
// rather than starting from an empty stack. The outermost `finally`'s own
// `return` is the one that wins, and it swallows a pending `throw` too.
// Every expectation here is Node's.

let log = "";

function note(s: string): void {
  log = log + s + ";";
}

function innerFinallyReturnRunsOuter(): string {
  try {
    try {
      return "inner";
    } finally {
      note("if");
      return "innerFinally";
    }
  } finally {
    note("of");
  }
}

function outermostFinallyReturnWins(): string {
  try {
    try {
      return "inner";
    } finally {
      return "innerFinally";
    }
  } finally {
    return "outer";
  }
}

function finallyReturnSwallowsThrow(): string {
  try {
    throw new Error("boom");
  } finally {
    return "swallowed";
  }
}

function threeLevels(): string {
  try {
    try {
      try {
        return "1";
      } finally {
        note("f3");
        return "r3";
      }
    } finally {
      note("f2");
    }
  } finally {
    note("f1");
  }
}

// A conditional `return` in the finally: when it isn't taken the pending
// `return` value from the try body has to survive the finally body.
function conditionalFinallyReturn(take: boolean): string {
  try {
    return "pending";
  } finally {
    note("fe");
    if (take) {
      return "taken";
    }
  }
}

function breakFromInsideFinally(): void {
  let i = 0;
  while (i < 3) {
    try {
      try {
        i = i + 1;
        continue;
      } finally {
        note("cf");
        if (i === 2) {
          break;
        }
      }
    } finally {
      note("lf");
    }
  }
}

function finallyReturnOverThrowRunsOuter(): string {
  try {
    try {
      throw new Error("x");
    } finally {
      return "fin";
    }
  } finally {
    note("og");
  }
}

function main(): void {
  log = "";
  assert(innerFinallyReturnRunsOuter() === "innerFinally", "inner finally return value");
  assert(log === "if;of;", "enclosing finally still runs after a return in a finally");

  log = "";
  assert(outermostFinallyReturnWins() === "outer", "outermost finally return wins");
  assert(log === "", "no notes on that path");

  assert(finallyReturnSwallowsThrow() === "swallowed", "finally return swallows a pending throw");

  log = "";
  assert(threeLevels() === "r3", "innermost finally return is the value");
  assert(log === "f3;f2;f1;", "all three finallys run, innermost first");

  log = "";
  assert(conditionalFinallyReturn(false) === "pending", "untaken finally return keeps the pending value");
  assert(log === "fe;", "finally body still ran");

  log = "";
  assert(conditionalFinallyReturn(true) === "taken", "taken finally return overrides");
  assert(log === "fe;", "finally body ran once");

  log = "";
  breakFromInsideFinally();
  assert(log === "cf;lf;cf;lf;", "break inside a finally runs the enclosing finally");

  log = "";
  assert(finallyReturnOverThrowRunsOuter() === "fin", "finally return replaces the throw");
  assert(log === "og;", "outer finally runs on the return-from-finally path");
}
