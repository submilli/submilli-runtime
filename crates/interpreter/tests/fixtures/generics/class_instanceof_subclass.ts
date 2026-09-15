// `instanceof` on a generic subclass. The runtime walk carries no type
// arguments, but a successful test against `Sub` on a `Box<number>` still
// proves `Sub<number>` — the target's parameters are solved from the operand,
// so the narrowed value keeps its member types.
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}

class Sub<T> extends Box<T> {
  extra: number;
  constructor(v: T) {
    super(v);
    this.extra = 9;
  }
}

function viaClass(x: Box<number>): number {
  if (x instanceof Sub) {
    return x.extra + x.v;
  }
  return 0;
}

function viaUnion(x: Box<number> | string): number {
  if (x instanceof Sub) {
    return x.extra;
  }
  return -1;
}

function main(): void {
  assert(viaClass(new Sub<number>(1)) === 10, "narrowed to the generic subclass");
  assert(viaClass(new Box<number>(1)) === 0, "base instance takes the else branch");
  assert(viaUnion(new Sub<number>(2)) === 9, "union member narrows");
  assert(viaUnion("s") === -1, "unrelated union member takes the else branch");
}
