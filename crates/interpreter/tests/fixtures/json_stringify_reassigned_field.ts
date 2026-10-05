// `JSON.stringify` serializes the value a field holds now, not the one its
// object literal was built with. The shape of `{ v: null }` types its field
// `null`, and of `{ v: 1 }` types it `number`; a binding with a wider type can
// later store a number or a string there. The same holds for array elements
// and for nested objects, and for an object assigned through a narrower field
// type, whose extra fields are still serialized. An Error subclass instance's
// `message` is not an own JSON property, and neither is its `name` until the
// instance sets one.

interface Box {
  v: number | null;
}

class CodedError extends Error {
  code: number = 7;
}

class NamedError extends Error {
  code: number = 8;
  constructor(message: string) {
    super(message);
    this.name = "NamedError";
  }
}

class Plain {
  x: number = 1;
  y: number = 2;
}

class Point {
  x: number = 1;
  y: number = 2;
}

function fill(box: Box): void {
  box.v = 5;
}

function one(): number {
  return 1;
}

function show(label: string, json: string, expected: string): void {
  console.log(label, json);
  assert(json === expected, label);
}

function main(): void {
  const num: { v: number | null } = { v: null };
  num.v = 5;
  show("number", JSON.stringify(num), "{\"v\":5}");

  const viaParam: Box = { v: null };
  fill(viaParam);
  show("through a parameter", JSON.stringify(viaParam), "{\"v\":5}");

  const lit: { v: 1 | 2 | null } = { v: null };
  lit.v = 2;
  show("number literal", JSON.stringify(lit), "{\"v\":2}");

  const str: { v: "a" | "b" | null } = { v: null };
  str.v = "b";
  show("string literal", JSON.stringify(str), "{\"v\":\"b\"}");

  const bool: { v: boolean | null } = { v: null };
  bool.v = false;
  show("boolean", JSON.stringify(bool), "{\"v\":false}");

  const mixed: { v: number | string } = { v: 1 };
  mixed.v = "s";
  show("number to string", JSON.stringify(mixed), "{\"v\":\"s\"}");

  const back: { v: number | null } = { v: 1 };
  back.v = null;
  show("back to null", JSON.stringify(back), "{\"v\":null}");

  const elems: { v: (number | null)[] } = { v: [null] };
  elems.v[0] = 3;
  show("array element", JSON.stringify(elems), "{\"v\":[3]}");

  const nested: { a: { b: string | null } } = { a: { b: null } };
  nested.a.b = "deep";
  show("nested field", JSON.stringify(nested), "{\"a\":{\"b\":\"deep\"}}");

  const obj: { v: { x: number } | null } = { v: null };
  obj.v = new Point();
  show("class instance", JSON.stringify(obj), "{\"v\":{\"x\":1,\"y\":2}}");

  const wide = { x: 1, y: 2 };
  const narrow: { v: { x: number } } = { v: { x: 0 } };
  narrow.v = wide;
  show("extra fields", JSON.stringify(narrow), "{\"v\":{\"x\":1,\"y\":2}}");

  const list: Box[] = [{ v: null }];
  list[0].v = 8;
  show("inside an array", JSON.stringify(list), "[{\"v\":8}]");

  const grown: { v: { x: number }[] } = { v: [{ x: 1 }] };
  grown.v.push(wide);
  show("extra fields in an element", JSON.stringify(grown), "{\"v\":[{\"x\":1},{\"x\":1,\"y\":2}]}");

  const pair: [number, number] = [1, 2];
  const longer: { v: number[] } = { v: pair };
  longer.v.push(3);
  show("a tuple grown as an array", JSON.stringify(longer), "{\"v\":[1,2,3]}");

  const many: { v: number | string }[] = [];
  for (let i = 0; i < 100; i++) {
    many.push({ v: i });
  }
  many[99].v = "last";
  const manyJson = JSON.stringify(many);
  show("a late mismatch", manyJson.substring(manyJson.length - 22), "{\"v\":98},{\"v\":\"last\"}]");

  const coded: { v: { code: number } } = { v: { code: 1 } };
  coded.v = new CodedError("boom");
  show("an Error in the field", JSON.stringify(coded), "{\"v\":{\"code\":7}}");

  // A function field keeps the object off the typed host walk.
  const withFn: { f: () => number; v: number | null } = { f: one, v: 1 };
  withFn.v = null;
  show("a function field, to null", JSON.stringify(withFn), "{\"v\":null}");
  withFn.v = 6;
  show("a function field, back", JSON.stringify(withFn), "{\"v\":6}");

  const plain: { v: { x: number } } = { v: { x: 0 } };
  plain.v = new Plain();
  show("a class with only fields", JSON.stringify(plain), "{\"v\":{\"x\":1,\"y\":2}}");

  const lateError: { e: { code: number } | null } = { e: null };
  lateError.e = new CodedError("late");
  show("an Error in a field that was null", JSON.stringify(lateError), "{\"e\":{\"code\":7}}");

  const named: { e: { code: number } | null } = { e: null };
  named.e = new NamedError("n");
  show("an Error that sets its name", JSON.stringify(named), "{\"e\":{\"code\":8,\"name\":\"NamedError\"}}");
  show("an Error directly", JSON.stringify(new CodedError("d")), "{\"code\":7}");

  const pretty: Box = { v: null };
  pretty.v = 4;
  show("pretty", JSON.stringify(pretty, null, 1), "{\n \"v\": 4\n}");
}
