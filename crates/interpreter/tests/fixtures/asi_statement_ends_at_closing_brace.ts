// JS terminates a statement before the `}` closing its block whether or not a newline
// intervenes, so every one-line body below is valid without its trailing `;`.
class C { x: number = 1 }

interface I { m(): number }

type T = { m(): number, x: number, opt?(): number };

enum E { A, B }

enum F {
  A
  B
}

function one(): number { return 1 }

function loops(): number {
  let n = 0
  for (let i = 0; i < 3; i = i + 1) { n = n + 1 }
  while (n < 4) { n = n + 1 }
  if (n === 4) { n = n + 1 } else { n = 0 }
  switch (n) { case 5: n = n + 1; break; default: n = 0 }
  return n
}

function main(): void {
  assert(one() === 1, "single-line function body needs no trailing `;`");
  assert(new C().x === 1, "single-line class body needs no trailing `;`");
  assert(loops() === 6, "single-line control-flow bodies need no trailing `;`");
  assert(E.A === 0 && E.B === 1, "single-line enum body");
  assert(F.A === 0 && F.B === 1, "newline-separated enum members take the `;` ASI inserts");

  const i: I = { m: (): number => 2 };
  assert(i.m() === 2, "single-line interface body");
  const t: T = { m: (): number => 3, x: 4, opt: (): number => 5 };
  assert(t.m() === 3 && t.x === 4, "a type literal accepts method members");
  assert(t.opt !== null && t.opt() === 5, "an optional type-literal method member");

  // A block sitting inside a call's parentheses is statement context again.
  const doubled = [1, 2, 3].map((v: number): number => {
    const out = v * 2
    return out
  })
  assert(doubled.length === 3 && doubled[2] === 6, "arrow block body inside a call gets ASI")

  const sum = [1, 2].reduce((acc: number, v: number): number => { return acc + v }, 0)
  assert(sum === 3, "one-line arrow block body inside a call")

  // A value list is never closed by an inserted `;`.
  const obj = { a: 1, b: 2 }
  assert(obj.a + obj.b === 3, "object literal is not terminated by ASI")
  const { a, b } = obj
  assert(a + b === 3, "destructuring pattern is not terminated by ASI")
  const nestedObj = { a: { b: { c: 1 } } }
  assert(nestedObj.a.b.c === 1, "nested object literals on one line")

  // A `:` introduces a value list everywhere except a `case`/`default` label, so a
  // ternary's alternate can be an object literal.
  const cond = true
  const picked = cond ? { a: 1 } : { a: 2 }
  assert(picked.a === 1, "object literal as a ternary alternate")
  const chained = pick(3)
  assert(chained.a === 3, "object literals in a chained ternary")
  assert(returnsAlternate(false).a === 2, "ternary alternate in a `return`")
  assert(field.v.a === 1, "ternary alternate in a class field initializer")

  // A `case` label's `:` is the one that introduces statements.
  let seen = 0
  switch (1) {
    case 1: { seen = seen + 1; break }
    default: { seen = seen + 10; break }
  }
  assert(seen === 1, "`case` and `default` bodies are statement blocks")

  // …but `case` and `default` are also legal property names, and a label can only
  // appear in a statement block, so these stay value lists.
  const keyworded: Keywords = {
    default: { a: 1 },
    case: 2,
    class: "c",
    interface: 5,
    enum: 6,
    do: 3,
    try: 4
  }
  assert(keyworded.default.a === 1 && keyworded.case === 2, "`default`/`case` as keys")
  // Read through `.` too: a property name ends an expression whatever the word is, so
  // the following line still gets its terminator.
  const viaClass = keyworded.class
  const viaDo = keyworded.do
  const viaTry = keyworded.try
  const after = 5
  assert(viaClass === "c" && viaDo + viaTry + after === 12, "keyword property reads")

  // A call's argument list is not a control-flow header just because the method is
  // spelled with a control-flow keyword.
  const router = new Router()
  const routed = router.switch(1)
  const looped = router.for(2)
  const caught = router.catch(3)
  const plain = 4
  assert(routed + looped + caught + plain === 11, "keyword-named method calls")

  // A body brace after the `>` closing a type-argument list is a block, not a value.
  assert(generics().length === 2, "generic return type, then a body brace")
  assert(new Box<number>().size() === 0, "generic class header, then a body brace")

  do { seen = seen + 1 } while (seen < 2)
  assert(seen === 2, "one-line do-while body, no trailing `;`")

  const acc = new Acc()
  acc.value = 5
  assert(acc.value === 5 && Acc.make().value === 0, "one-line accessors and statics")
}

class Acc {
  private v: number = 0
  get value(): number { return this.v }
  set value(n: number) { this.v = n }
  static make(): Acc { return new Acc() }
}

class Field { v: { a: number } = true ? { a: 1 } : { a: 2 } }
const field = new Field()

// Every word here is a keyword and a legal property name. (`else`, `catch`, `finally`,
// `extends`, and `implements` are too, but not when written on their own line:
// `can_continue` reads them as continuations — a separate, pre-existing defect.)
interface Keywords {
  default: { a: number }
  case: number
  class: string
  interface: number
  enum: number
  do: number
  try: number
}

class Router {
  switch(n: number): number { return n + 1 }
  for(n: number): number { return n }
  if(n: number): number { return n }
  while(n: number): number { return n }
  catch(n: number): number { return n }
}

function generics(): Array<number> {
  const a = [1, 2]
  return a
}

class Box<T> {
  private readonly seen: Map<string, T> = new Map<string, T>()

  size(): number {
    return this.seen.size
  }
}

function pick(n: number): { a: number } {
  return n === 1 ? { a: 1 } : n === 2 ? { a: 2 } : { a: 3 }
}

function returnsAlternate(c: boolean): { a: number } {
  return c ? { a: 1 } : { a: 2 }
}
