// A `const` bound to a `?:` whose branches are all literals of one primitive type
// keeps them, as TypeScript does: `cond ? "a" : "b"` is `"a" | "b"`, so it reaches
// a literal-union parameter. Branches of different primitives widen. A `let`
// copying such a `const` widens to the primitive, while a `let` from a value
// already typed with literals keeps its initial narrowing. Literal-typed values
// still behave as their primitive: they iterate as strings and stringify as
// numbers, at module level too.
enum Level {
  Low,
  High,
}
enum Dir {
  Up = "up",
}

function pick(): boolean {
  return "ab".length === 2;
}
const cond = pick();
const moduleSide = cond ? 3 : 4;
const moduleLabel = cond ? "left" : "right";
const pinned: 123 | 456 = 123;

function side(s: "left" | "right"): number {
  return s === "left" ? 1 : 2;
}
function small(n: 1 | 2 | 3): number {
  return n;
}
function word(w: "x" | "y" | "z"): string {
  return w;
}

function main(): void {
  const s = cond ? "left" : "right";
  assert(side(s) === 1, "a ternary of string literals keeps both");
  assert(side(moduleLabel) === 1, "so does a module-level one");
  const n = cond ? (cond ? 1 : 2) : 3;
  assert(small(n) === 1, "nested ternaries of number literals keep all three");
  const w = (cond ? "x" : "y");
  assert(word(w) === "x", "parentheses keep them too");

  let copied = s;
  assert(copied !== "up", "comparing with \"up\" compiles: the copy is `string`");
  copied = "up";
  assert(copied === "up", "and can be reassigned");
  const mixed = cond ? "on" : 0;
  let mixedCopy = mixed;
  assert(mixedCopy !== "off", "mixed branches widen, so the copy starts out unnarrowed");

  // A `let` from a value already typed with literals keeps the narrowing it
  // starts with, so it reaches a literal-union parameter.
  const sides = new Map<string, "left" | "right">();
  sides.set("k", "left");
  let looked = sides.get("k");
  let reached = 0;
  if (looked) {
    reached += side(looked);
  }
  const all: ("left" | "right")[] = ["right"];
  let found = all.find((v) => v === "right");
  if (found) {
    reached += side(found);
  }
  assert(reached === 3, "a `let` keeps its initial narrowing");

  assert(JSON.stringify(moduleSide) === "3", "a module-level number-literal union stringifies");
  assert(JSON.stringify(pinned) === "123", "so does an annotated one");
  assert(JSON.stringify(Level.High) === "1", "a number enum still stringifies");
  assert(`${moduleSide}` === "3", "and interpolates");

  let letters = "";
  for (const ch of s) {
    letters += ch;
  }
  for (const ch of moduleLabel) {
    letters += ch;
  }
  const single = "ab";
  for (const ch of single) {
    letters += ch;
  }
  for (const ch of Dir.Up) {
    letters += ch;
  }
  assert(letters === "leftleftabup", "literal strings and string enums iterate by character");
}
