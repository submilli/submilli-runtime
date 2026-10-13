interface Point { x: number }

function describe(value: {}): string {
  if (typeof value === "number") {
    return "number " + String(value + 1);
  }
  if (typeof value === "string") {
    return "string " + String(value.length);
  }
  if (typeof value === "boolean") {
    return value ? "true" : "false";
  }
  return "object " + String(JSON.stringify(value));
}

function same(left: {}, right: {}): boolean {
  return left === right;
}

function main(): void {
  const n: {} = 1;
  const s: {} = "hi";
  const b: {} = false;
  const o: {} = { x: 1 };
  const big: {} = 5n;
  assert(describe(n) === "number 2", "a number in `{}` keeps its type tag");
  assert(describe(s) === "string 2", "a string in `{}` keeps its type tag");
  assert(describe(b) === "false", "a boolean in `{}` keeps its type tag");
  assert(describe(o) === "object {\"x\":1}", "an object in `{}` is still an object");
  assert(typeof big === "bigint", "a bigint in `{}` keeps its type tag");
  assert(same(n, 1) && same(s, "hi") && !same(n, s), "`===` compares the held values");
  assert(`${n}-${s}-${b}` === "1-hi-false", "templates format the held values");

  const zero: {} = 0;
  assert(!zero && !!n, "a falsy primitive in `{}` is falsy");

  const values: {}[] = [1, "a", true, { x: 2 }];
  assert(JSON.stringify(values) === "[1,\"a\",true,{\"x\":2}]", "an array of `{}` holds mixed values");

  const point = o as Point;
  assert(point.x === 1, "a cast out of `{}` reads the object");
  assert(JSON.stringify({ ...o }) === "{\"x\":1}", "spreading `{}` copies the object's fields");
  assert(JSON.stringify({ ...n }) === "{}", "spreading a number in `{}` copies nothing");

  const andZero = zero && "and";
  assert(andZero === 0, "`&&` on a falsy primitive in `{}` gives the primitive");
  const fn: {} = () => 1;
  assert(JSON.stringify(fn) === undefined, "a function in `{}` has no JSON text");

  assert(castThrows("null"), "a cast to `{}` rejects `null`");
  assert(castThrows("[1,null]", true), "a cast to `{}[]` rejects a `null` element");
  assert(!castThrows("5"), "a cast to `{}` accepts a number");

  const maybe: {} | null | undefined = 0;
  assert((maybe || "fallback") === "fallback", "`{} | null | undefined` can be tested");
  assert(testedAfterJoin(0) === "falsy" && testedAfterJoin(null) === "null", "a join keeps `{} | null | undefined`");
  const parsed: unknown = JSON.parse("0");
  const anyValue: {} | null | undefined = parsed;
  assert(anyValue === 0, "`unknown` fits `{} | null | undefined`");
  assert(castThrows("{\"a\":null}", false, true), "a cast to `{ a: {} }` rejects a `null` field");

  let changing: {} = 1;
  changing = "x";
  assert(changing === "x", "a `{}` variable takes values of any type");
}

function testedAfterJoin(value: {} | null | undefined): string {
  if (value === null) {
    return "null";
  }
  return value ? "truthy" : "falsy";
}

function castThrows(json: string, asArray: boolean = false, asField: boolean = false): boolean {
  try {
    if (asField) {
      JSON.parse(json) as { a: {} };
    } else if (asArray) {
      JSON.parse(json) as {}[];
    } else {
      JSON.parse(json) as {};
    }
    return false;
  } catch (e) {
    return true;
  }
}
