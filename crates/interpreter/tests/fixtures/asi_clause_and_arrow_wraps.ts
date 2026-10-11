// Prettier/LLM line-wrap shapes ASI has to hold together: a clause keyword on its
// own line after a block's `}`, and a body wrapped after `=>`.
function main(): void {
  const arrow = (x: number): number =>
    x * 2;
  assert(arrow(3) === 6, "arrow body wrapped after `=>`");

  const long = (a: number, b: number): number =>
    a +
    b;
  assert(long(1, 2) === 3, "arrow body wrapped across several lines");

  let trace = "";
  try {
    trace = trace + "t";
  }
  catch (e) {
    trace = trace + "c";
  }
  assert(trace === "t", "`catch` on its own line after `}`");

  try {
    throw new Error("boom");
  }
  catch (e) {
    trace = trace + "C";
  }
  finally {
    trace = trace + "f";
  }
  assert(trace === "tCf", "`catch` and `finally` both on their own lines");

  try {
    trace = trace + "y";
  }
  finally {
    trace = trace + "F";
  }
  assert(trace === "tCfyF", "`finally` alone on its own line");

  // The body brace of an arrow stays a statement block, not a value list.
  const block = (n: number): number => {
    const doubled = n * 2
    return doubled
  };
  assert(block(4) === 8, "arrow block body keeps its statement terminators");

  const wrappedBlock = (n: number): number =>
  {
    return n * 3;
  };
  assert(wrappedBlock(2) === 6, "block body on the line after `=>`");

  const curried = (a: number): ((b: number) => number) =>
    (b: number): number =>
      a + b;
  assert(curried(1)(2) === 3, "curried arrow wrapped after each `=>`");

  const chained = [1, 2, 3]
    .map((n: number): number => n * 2)
    .filter((n: number): boolean => n > 2);
  assert(chained.length === 2, "member chain wrapped before `.`");

  const maybe: Box | null = null as Box | null;
  const reached = maybe
    ?.v;
  assert(reached === undefined, "member chain wrapped before `?.`");

  const sub = new Sub();
  assert(sub.tag() === "sub", "class header wrapped before `extends`");
  const impl = new Impl();
  assert(impl.v === 4, "class header wrapped before `implements`");
}

interface Box {
  v: number;
}

class Base {
  tag(): string {
    return "base";
  }
}

class Sub
  extends Base {
  tag(): string {
    return "sub";
  }
}

class Impl
  implements Box {
  v: number = 4;
}
