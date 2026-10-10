// Class universal toJson (vtable slot 1): declared data fields as a JSON
// object in canonical sorted-key order (subclass fields sort into the parent's
// — payload order is inherited-prefix-then-own, an ABI, not the output order),
// nullable fields print null, method closures are excluded, nesting works in
// both directions, and a user `toJson(): string` method overrides the default.
class Point {
  x: number;
  y: number;

  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

class Tagged extends Point {
  a: string;

  constructor(x: number, y: number, a: string) {
    super(x, y);
    this.a = a;
  }
}

class MaybeNum {
  v: number | null;

  constructor(v: number | null) {
    this.v = v;
  }
}

class Wrapped {
  p: Point;
  label: string;

  constructor(p: Point, label: string) {
    this.p = p;
    this.label = label;
  }
}

class Redacted {
  secret: number;

  constructor(secret: number) {
    this.secret = secret;
  }

  toJson(): string {
    return "\"redacted\"";
  }
}

function main(): void {
  assert(JSON.stringify(new Point(1, 2)) === "{\"x\":1,\"y\":2}");
  const u: unknown = new Point(1, 2);
  assert(JSON.stringify(u) === "{\"x\":1,\"y\":2}");
  assert(new Point(1, 2).toJson() === "{\"x\":1,\"y\":2}");

  assert(JSON.stringify(new Tagged(1, 2, "t")) === "{\"a\":\"t\",\"x\":1,\"y\":2}");

  assert(JSON.stringify(new MaybeNum(null)) === "{\"v\":null}");
  assert(JSON.stringify(new MaybeNum(7)) === "{\"v\":7}");

  assert(
    JSON.stringify(new Wrapped(new Point(1, 2), "w")) ===
      "{\"label\":\"w\",\"p\":{\"x\":1,\"y\":2}}",
  );
  const holder = { inner: new Point(3, 4) };
  assert(JSON.stringify(holder) === "{\"inner\":{\"x\":3,\"y\":4}}");
  const arr: Point[] = [new Point(1, 2), new Point(3, 4)];
  assert(JSON.stringify(arr) === "[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}]");

  assert(JSON.stringify(new Redacted(42)) === "\"redacted\"");
  const ru: unknown = new Redacted(42);
  assert(JSON.stringify(ru) === "\"redacted\"");
}
