// A generic or `unknown` value holding null interpolates as "null", however the
// value reaches the template: through generic calls, interface methods,
// closures, destructuring, conditionals, callbacks, maps and records.
interface Labelled {
  a: number;
  toJson?: () => string;
  toString?: () => string;
}

interface Getter<T> {
  get(): T;
}

class Cell<T> implements Getter<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  get(): T {
    return this.v;
  }
}

function innerText<T>(x: T): string {
  return `<${x}>`;
}

function outerText<T>(x: T): string {
  return innerText(x);
}

function readThrough<T>(g: Getter<T>): string {
  return `${g.get()}`;
}

function laterText<T>(x: T): string {
  const read = (): T => x;
  return `${read()}`;
}

function pairText<T>(xs: T[], pair: [T, number]): string {
  const [first] = xs;
  const [held, n] = pair;
  return `${first}|${held}${n}`;
}

function choiceText<T>(pick: boolean, x: T, y: T): string {
  return `${pick ? x : y}`;
}

function mappedText<T>(xs: T[]): string {
  return xs.map((x) => `(${x})`).join("");
}

function guardedText<T>(x: T): string {
  try {
    return `${x}`;
  } catch (e) {
    return (e as Error).message;
  }
}

class Slot<T> {
  v: T;
  label: string;
  constructor(v: T) {
    this.v = v;
    this.label = `L${v}`;
  }
  get text(): string {
    return `g${this.v}`;
  }
  static of<U>(u: U): string {
    return `s${u}`;
  }
}

const absent: unknown = null;
const absentText = `top${absent}`;

function nestedText<T>(x: T): string {
  return `a${`b${x}c`}d`;
}

function depthText<T>(x: T, n: number): string {
  return n === 0 ? `${x}` : `(${depthText(x, n - 1)}${x})`;
}

function manyText<A, B>(a: A, b: B, u: unknown): string {
  return `plain${a}${b}${u}|${a}${a}${a}${b}${b}${b}${u}${u}${u}`;
}

function main(): void {
  assert(outerText<Labelled | null>(null) === "<null>", "a generic null passed through a generic call");
  assert(readThrough<Labelled | null>(new Cell<Labelled | null>(null)) === "null", "a generic null from an interface method");
  assert(laterText<Labelled | null>(null) === "null", "a generic null returned by a closure");
  assert(pairText<string | null>([null], [null, 1]) === "null|null1", "generic nulls destructured from an array and a tuple");
  assert(choiceText<Labelled | null>(false, { a: 1 }, null) === "null", "a generic null chosen by a conditional");
  assert(mappedText<Labelled | null>([null, { a: 1 }]) === "(null)([object Object])", "generic nulls in a map callback");
  const failing: Labelled = {
    a: 1,
    toString: () => {
      throw new Error("boom");
    },
  };
  assert(guardedText<Labelled | null>(failing) === "boom" && guardedText<Labelled | null>(null) === "null", "a generic conversion that throws stays catchable");
  const byKey = new Map<string, unknown>([["k", null]]);
  const record: Record<string, unknown> = { k: null };
  assert(`${byKey.get("k")}|${record["k"]}` === "null|null", "an unknown null from a map or a record");
  const slot = new Slot<Labelled | null>(null);
  assert(
    `${slot.label}|${slot.text}|${Slot.of<Labelled | null>(null)}` === "Lnull|gnull|snull",
    "a generic null in a constructor, a getter and a static method",
  );
  assert(absentText === "topnull", "an unknown null in a top-level const");
  assert(nestedText<string | null>(null) === "abnullcd", "a generic null in a nested template");
  assert(depthText<Labelled | null>(null, 2) === "((nullnull)null)", "a generic null through recursion");
  assert(
    manyText<string | null, Labelled | null>(null, null, null) ===
      "plainnullnullnull|nullnullnullnullnullnullnullnullnull",
    "many generic and unknown substitutions in one template",
  );
}
