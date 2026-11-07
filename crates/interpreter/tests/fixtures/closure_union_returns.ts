function main(): void {
  const pick = (x: number) => {
    if (x > 0) return { p: "a", n: x };
    return { p: "b", error: "neg" };
  };
  assert(pick(1).n === 1, "positive object return");
  assert(pick(-1).n === undefined, "missing property normalizes");
  const negative = pick(-1);
  if (negative.error !== undefined) {
    assert(negative.error.length === 3, "narrow optional error");
  }
  const primitive = (x: number) => {
    if (x > 0) return 1;
    return "neg";
  };
  const exact: 1 | "neg" = primitive(-1);
  assert(exact === "neg", "mixed literals keep their types");
  assert(primitive(1) === 1, "number union return");
  const expression = function(x: number) {
    if (x > 0) return { n: x };
    return { error: "neg" };
  };
  assert(expression(2).n === 2, "function expression");
  const results = [1, -1].map(x => {
    try {
      if (x < 0) throw new Error("negative");
      return { ok: true, n: x };
    } catch (e) {
      return { ok: false, error: String(e) };
    }
  });
  assert(results[0].n === 1, "map success");
  assert(results[1].error !== undefined, "map catch");
  const ternary = (x: number) => x > 0 ? { n: x } : { error: "neg" };
  assert(ternary(-1).n === undefined, "ternary parity");
  const contextual: (x: number) => number | string = x => {
    if (x > 0) return x;
    return "neg";
  };
  assert(contextual(1) === 1, "contextual union");
  const mixed = (x: number) => {
    if (x > 0) return x;
    return "neg";
  };
  const mixedExact: number | "neg" = mixed(-1);
  assert(mixedExact === "neg", "literal beside a nonliteral");
  const maybe = (x: number) => {
    if (x === 1) return { a: x };
    if (x === 2) return { b: "two" };
    if (x === 3) return 0;
    return undefined;
  };
  const maybeExact: { a?: number; b?: string } | 0 | undefined = maybe(3);
  assert(maybeExact === 0, "literal beside objects");
  const object = maybe(2);
  if (object !== undefined && typeof object !== "number") {
    assert(object.a === undefined, "primitive peers preserve object normalization");
    assert(object.b === "two", "normalized object keeps its field");
  }
  const primitivePeers = (x: number) => {
    if (x === 1) return { a: x };
    if (x === 2) return { b: "two" };
    if (x === 3) return 1n;
    return x > 4 ? 0 : "none";
  };
  const peer = primitivePeers(2);
  if (typeof peer === "object") {
    assert(peer.a === undefined, "primitive union and bigint peers normalize");
    assert(peer.b === "two", "object fields survive primitive peers");
  }
}
