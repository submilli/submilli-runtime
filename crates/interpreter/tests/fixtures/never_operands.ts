// A `never` operand is accepted by `+`, arithmetic, ordering and unary `-`/`+`:
// no value of it exists, so the operator takes its result from the other
// operand, as tsc does. Each use below either sits in a dead branch or
// evaluates an expression that throws first.
type Shape = { kind: "circle"; r: number } | { kind: "square"; side: number };

function assertNever(x: never): never {
  throw new Error("Unexpected: " + x);
}

function area(s: Shape): number {
  switch (s.kind) {
    case "circle":
      return 3 * s.r * s.r;
    case "square":
      return s.side * s.side;
    default:
      return assertNever(s);
  }
}

function fail(message: string): never {
  throw new Error(message);
}

enum Level { Low = 1, High = 2 }

let total = 0;

class Counter {
  count: number = 0;
}

function label(k: "a" | "b"): string {
  if (k === "a") return "A";
  if (k === "b") return "B";
  return "unexpected " + k;
}

function main(): void {
  assert(area({ kind: "circle", r: 1 }) === 3, "circle area");
  assert(area({ kind: "square", side: 2 }) === 4, "square area");
  assert(label("a") + label("b") === "AB", "labels");

  let caught = 0;
  try { const s: string = "a" + fail("concat"); console.log(s); } catch (e) { caught++; }
  try { const n: number = fail("mul") * 2; console.log(n); } catch (e) { caught++; }
  try { const b: bigint = 1n - fail("bigint"); console.log(b); } catch (e) { caught++; }
  try { const lt: boolean = 1n < fail("ordering"); console.log(lt); } catch (e) { caught++; }
  try { const neg: number = -fail("negate"); console.log(neg); } catch (e) { caught++; }
  try { const s: string = fail("left concat") + "s"; console.log(s); } catch (e) { caught++; }
  try { const n: number = fail("both") + fail("other"); console.log(n); } catch (e) { caught++; }
  try { const n: number = +fail("unary plus"); console.log(n); } catch (e) { caught++; }
  try { const n: number = Level.High % fail("enum"); console.log(n); } catch (e) { caught++; }
  try { const n: number = fail("div") / 2 + 2 ** fail("pow"); console.log(n); } catch (e) { caught++; }
  try { if (fail("condition") > 0) { console.log("then"); } } catch (e) { caught++; }
  assert(caught === 11, "every operator evaluates its throwing operand");

  let order = "";
  const mark = (c: string): number => { order += c; return 1; };
  try { const n: number = mark("L") - fail("rhs"); console.log(n); } catch (e) { order += "!"; }
  assert(order === "L!", "the left operand is evaluated before a throwing right operand");

  let text = "x";
  try { text += fail("compound"); } catch (e) { text += "!"; }
  const counts: number[] = [1];
  try { counts[0] += fail("indexed"); } catch (e) { counts[0] = counts[0] + 1; }
  assert(text === "x!" && counts[0] === 2, "compound assignments throw before writing");

  const counter = new Counter();
  let captured = 1;
  const bump = (): void => { captured *= fail("captured"); };
  try { total -= fail("global"); } catch (e) { total = 7; }
  try { counter.count += fail("field"); } catch (e) { counter.count = 5; }
  try { bump(); } catch (e) { captured = 3; }
  assert(total === 7 && counter.count === 5 && captured === 3, "every compound target throws first");
}
