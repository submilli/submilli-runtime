// `finally` runs exactly once on every path through a multi-catch try:
// no throw, first arm, second arm, and unmatched re-raise (before the outer
// handler sees the error).
class AError extends Error {
  constructor() {
    super("a");
    this.name = "AError";
  }
}

class BError extends Error {
  constructor() {
    super("b");
    this.name = "BError";
  }
}

class CError extends Error {
  constructor() {
    super("c");
    this.name = "CError";
  }
}

let trail = "";

function run(which: number): void {
  try {
    try {
      if (which === 1) {
        throw new AError();
      }
      if (which === 2) {
        throw new BError();
      }
      if (which === 3) {
        throw new CError();
      }
      trail = trail + "body;";
    } catch (e: AError) {
      trail = trail + "armA;";
    } catch (e: BError) {
      trail = trail + "armB;";
    } finally {
      trail = trail + "fin;";
    }
  } catch (e) {
    trail = trail + "outer:" + e.name + ";";
  }
}

function main(): void {
  run(0);
  run(1);
  run(2);
  run(3);
  assert(
    trail === "body;fin;armA;fin;armB;fin;fin;outer:CError;",
    "finally runs once per path, before the outer handler on re-raise"
  );
}
