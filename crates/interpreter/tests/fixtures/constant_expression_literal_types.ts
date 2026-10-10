// TypeScript gives some constant expressions a literal type, and so do we:
// `!` on a value of known truthiness is `true` or `false`, and a template
// whose substitutions are constants is the string it spells. A `let` widens.

enum Level {
  Low = 1,
  High = 2,
}

type Greeting = "hello world";

function never(): string {
  return "unreachable";
}

function main(): void {
  const f: false = !true;
  const t: true = !!true;
  const zero: true = !0;
  const blank = "";
  const empty: true = !blank;
  const obj = { a: 1 };
  const present: false = !obj;
  assert(!f && t && zero && empty && !present, "`!` folds as TypeScript does");

  let widened = !true;
  widened = true;
  assert(widened, "a `let` widens `!true` to boolean");

  const world = "world";
  const g: Greeting = `hello ${world}`;
  assert(g === "hello world", "a template of a `const` literal");

  const n: "abc0abc" = `abc${0}abc`;
  assert(n === "abc0abc", "a template of a number");
  const sum: "3" = `${1 + 2}`;
  const neg: "a-1" = `a${-1}`;
  const level: "level 2" = `level ${Level.High}`;
  assert(sum === "3" && neg === "a-1" && level === "level 2", "arithmetic and enum members");

  switch (`abc${0}abc`) {
    case `abc${0}abc`:
      assert(true, "a constant template is a `case` label");
      break;
  }

  let text = `x${1}`;
  text = text + "y";
  assert(text === "x1y", "a `let` widens a constant template to string");

  // `!` on a value whose truthiness the type doesn't decide stays boolean.
  const s = never();
  const b: boolean = !s;
  assert(!b, "`!` on a string is boolean");
}
