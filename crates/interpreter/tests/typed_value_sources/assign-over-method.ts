// Assigning to a prototype method or Object/prelude member creates an own
// property on the receiver without affecting other instances.
class Base {
  a: number = 1;

  m(): number {
    return 1;
  }

  call(): number {
    return this.m() * 10;
  }
}

class Derived extends Base {
  extra: number = 2;

  overrideViaSuper(f: () => number): void {
    super.m = f;
  }
}

class WithAccessor {
  writes: number = 0;

  get x(): number {
    return this.writes;
  }

  set x(v: number) {
    this.writes = v;
  }
}

function keysViaForIn(o: object): string {
  const out: string[] = [];
  for (const k in o) {
    out.push(k);
  }
  return out.join(",");
}

export function main(): string {
  const b1 = new Base();
  const b2 = new Base();

  b1.m = () => 99;
  const firstWrite = [
    String(b1.m()),
    String(b1.call()),
    String(b2.m()),
    String(b2.call()),
    String(Object.hasOwn(b1, "m")),
    String(Object.hasOwn(b2, "m")),
    String("m" in b1),
    String("m" in b2),
    Object.keys(b1).join(","),
    keysViaForIn(b1),
    Object.keys({ ...b1 }).join(","),
  ];

  b1.m = () => 77;
  const secondWrite = [String(b1.call()), Object.keys(b1).join(",")];

  b1.toString = () => "custom-b1";
  const toStringWrite = [
    b1.toString(),
    String(b1),
    b2.toString(),
    String(b2),
    String(Object.hasOwn(b1, "toString")),
    String(Object.hasOwn(b2, "toString")),
    Object.keys(b1).join(","),
    JSON.stringify(b1),
  ];

  const e1 = new Error("boom");
  const e2 = new Error("ok");
  e1.toString = () => "custom-err";
  const errorWrite = [
    e1.toString(),
    String(e1),
    String(e2),
    String(Object.hasOwn(e1, "toString")),
    String(Object.hasOwn(e2, "toString")),
  ];

  const s1 = new Derived();
  const s2 = new Derived();
  s1.overrideViaSuper(() => 55);
  const superWrite = [
    String(s1.m()),
    String(s1.call()),
    String(s2.call()),
    String(Object.hasOwn(s1, "m")),
    Object.keys(s1).join(","),
  ];

  const acc = new WithAccessor();
  acc.x = 42;
  const accWrite = [String(acc.x), String(Object.hasOwn(acc, "x"))];

  const ordered: Record<string, number> = {};
  ordered["b"] = 1;
  ordered["2"] = 2;
  ordered["a"] = 3;
  ordered["0"] = 4;
  const keyOrder = [
    Object.keys(ordered).join(","),
    keysViaForIn(ordered),
    Object.keys({ ...ordered }).join(","),
  ];

  return [
    firstWrite.join("|"),
    secondWrite.join("|"),
    toStringWrite.join("|"),
    errorWrite.join("|"),
    superWrite.join("|"),
    accWrite.join("|"),
    keyOrder.join("|"),
  ].join(" // ");
}
