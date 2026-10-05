// A `let` widens a literal type only when it is fresh, as TypeScript does: one
// written as a literal, or copied from a `const` bound to one. A literal type
// from an annotation, an assertion, a declared field or a narrowing of a
// declared type stays, so the copy reaches a literal-typed parameter. An object
// literal's property follows the same rule. A fresh literal also doesn't
// narrow a binding whose declared type doesn't name it.
//
// A literal type that may have come from a fresh literal widens: a template,
// a generic call's result, an assignment's narrowing of an object type.
interface Shape {
  kind: "circle" | "square";
  size: 1 | 2;
}

function pick(): boolean {
  return "ab".length === 2;
}
const cond = pick();
const moduleFresh = "m";
const moduleDeclared: "m" = "m";
let moduleCopy = moduleDeclared;

function hello(h: "hello"): number {
  return h.length;
}
function circle(k: "circle"): number {
  return k.length;
}
function module(m: "m"): number {
  return m.length;
}
function small(n: 1 | 2): number {
  return n;
}
function emptyOrHello(v: "" | "hello"): number {
  return v.length;
}

class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
class Named {
  name(): "named" {
    return "named";
  }
}

function main(): void {
  const fresh = "hello";
  const declared: "hello" = "hello";
  const chained = declared;
  const asserted = "hello" as "hello";

  let fromDeclared = declared;
  let fromChain = chained;
  let fromAssertion = asserted;
  let fromMixed = cond ? declared : fresh;
  assert(
    hello(fromDeclared) + hello(fromChain) + hello(fromAssertion) + hello(fromMixed) === 20,
    "a copy of a regular literal type keeps it",
  );
  assert(module(moduleCopy) === 1, "at module level too");

  let fromFresh = fresh;
  fromFresh = "other";
  let fromModuleFresh = moduleFresh;
  fromModuleFresh = "other";
  assert(fromFresh === "other" && fromModuleFresh === "other", "a fresh copy widens");

  const shape: Shape = { kind: "circle", size: 2 };
  let kind = shape.kind;
  let size = shape.size;
  assert(small(size) === 2, "a declared field's literal type stays");
  if (kind === "circle") {
    let narrowed = kind;
    assert(circle(narrowed) === 6, "a narrowing of a declared type stays");
  }
  const named = new Named();
  let name = named.name();
  assert(name.length === 5 && hello(declared) === 5, "a method's declared result stays");
  let text: string = pick() ? "abc" : "";
  let either = text && declared;
  assert(emptyOrHello(either) === 5, "`&&` keeps the `\"\"` of a `string`");

  const ternary = cond ? "a" : "b";
  if (ternary === "a") {
    let stillFresh = ternary;
    stillFresh = "z";
    assert(stillFresh === "z", "a narrowing of a fresh literal widens");
  }

  const props = { kept: declared, widened: fresh };
  props.widened = "other";
  assert(hello(props.kept) === 5 && props.widened === "other", "properties follow the same rule");

  let nullable: string | null = fresh;
  assert(nullable !== "other", "a fresh literal doesn't narrow a `string` binding");

  const yes = true;
  let maybe: boolean | null = yes;
  let copied = maybe;
  copied = false;
  const holder = { flag: maybe };
  holder.flag = false;
  assert(!copied && !holder.flag, "a fresh `true` narrowing a `boolean | null` widens");

  let template = `${declared}`;
  template = "q";
  const counts: number[] = [1, 2];
  let first = counts.map(() => fresh)[0];
  first = "b";
  let boxed = new Box(fresh).value;
  boxed = "c";
  assert(template + first + boxed === "qbc", "a template or generic result widens");

  let assigned: { key: string } | null = null;
  assigned = { key: fresh };
  if (assigned !== null) {
    let key = assigned.key;
    key = "d";
    assert(key === "d", "an assignment's narrowing to an object type widens");
  }
}
