// A field typed by an enum discriminates a union as a field typed by literals
// does, a tuple element that holds `null` discriminates a union of tuples,
// and equality with a variable keeps only what the variable's type allows.
enum E {
  A = "a",
  B = "b",
}

enum F {
  X = "x",
  Y = "y",
}

interface WithE {
  kind: E;
  a: number;
}

interface WithF {
  kind: F;
  b: string;
}

function bySwitch(u: WithE | WithF): string {
  switch (u.kind) {
    case E.A:
    case E.B:
      return "e";
    default:
      return u.b;
  }
}

function byEquality(u: WithE | WithF): string {
  if (u.kind === F.Y) return u.b;
  return "not F.Y";
}

type Pair = [null, string] | [boolean, number];

function byNullElement(v: Pair): string {
  if (v[0] === null) return v[1];
  return String(v[1] + 1);
}

function equalToVariable(e: E | null, other: E): E {
  if (e !== other) return other;
  return e;
}

function main(): void {
  assert(bySwitch({ kind: F.Y, b: "bee" }) === "bee");
  assert(bySwitch({ kind: E.A, a: 1 }) === "e");
  assert(byEquality({ kind: F.Y, b: "y" }) === "y");
  assert(byNullElement([null, "s"]) === "s");
  assert(byNullElement([true, 2]) === "3");
  assert(equalToVariable(null, E.B) === E.B);
  assert(equalToVariable(E.A, E.A) === E.A);
  console.log("ok");
}
