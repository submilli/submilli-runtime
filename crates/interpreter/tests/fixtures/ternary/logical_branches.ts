// A `&&` or `||` whose left side is narrowed to always-truthy or always-falsy
// keeps only its right side's type: under `c === true`, `c && n()` is `number`.
// The left side is still evaluated and tested, and the right side's value is
// the result.
let log: string[] = [];

function n(): number {
  log.push("n");
  return 1;
}

function f(): void {
  log.push("f");
}

function flag(value: boolean): boolean {
  log.push("flag");
  return value;
}

function taken(): string {
  const joined = log.join(",");
  log = [];
  return joined;
}

function describe(value: boolean | number | string): string {
  if (typeof value === "number") {
    return "number:" + value.toString();
  }
  if (typeof value === "boolean") {
    return "boolean:" + value.toString();
  }
  return "string:" + value;
}

function bound(c: boolean): string {
  const andThen = c ? c && n() : 2;
  const orElse = c ? 3 : c || n();
  const andElse = c ? 4 : c && n();
  const orThen = c ? c || n() : 5;
  return [describe(andThen), describe(orElse), describe(andElse), describe(orThen)].join(" ");
}

function returned(c: boolean): number {
  return c ? c && n() : 2;
}

function nested(c: boolean, d: boolean): string {
  const value = c ? (d ? c && d && n() : c && 6) : d ? c || d || n() : 7;
  return describe(value);
}

function literal(k: "a" | "", zero: 0 | 1): string {
  const fromString = k ? k && n() : k || 8;
  const fromNumber = zero ? zero && "one" : zero || "zero";
  return describe(fromString) + " " + describe(fromNumber);
}

function main(): void {
  const yes: boolean = ["a"].length === 1;
  const no: boolean = !yes;

  yes ? yes && n() : 2;
  no ? 2 : no || n();
  yes ? yes && n() : f();
  no ? f() : no || n();
  assert(taken() === "n,n,n,n", "statements run the right side");

  assert(bound(yes) === "number:1 number:3 number:4 boolean:true", "bound, condition true");
  assert(taken() === "n", "bound, condition true: calls");
  assert(bound(no) === "number:2 number:1 boolean:false number:5", "bound, condition false");
  assert(taken() === "n", "bound, condition false: calls");

  assert(returned(yes) === 1 && returned(no) === 2, "returned");
  assert(describe(yes ? yes && n() : 2) === "number:1", "argument");
  taken();

  assert(nested(yes, yes) === "number:1", "nested, both true");
  assert(nested(yes, no) === "number:6", "nested, first true");
  assert(nested(no, yes) === "boolean:true", "nested, second true");
  assert(nested(no, no) === "number:7", "nested, both false");
  taken();

  assert(literal("a", 1) === "number:1 string:one", "truthy literals");
  assert(literal("", 0) === "number:8 string:zero", "falsy literals");
  taken();

  if (yes) {
    const underIf = yes && n();
    assert(describe(underIf) === "number:1", "narrowed by if, &&");
  }
  if (!no) {
    const underIf = no || n();
    assert(describe(underIf) === "number:1", "narrowed by if, ||");
  }
  taken();

  // The narrowed left side is still evaluated once when it has effects.
  const kept = flag(yes) && yes ? yes && n() : 0;
  assert(kept === 1 && taken() === "flag,n", "left side with effects");
}
