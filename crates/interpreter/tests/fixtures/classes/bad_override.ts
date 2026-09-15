// expect-error: override of method `value` is not compatible
// expect-error: override of method `read` is not compatible
class Base {
  value(): number {
    return 1;
  }
}

class Sub extends Base {
  value(): string {
    return "x";
  }
}

class GenericBase<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
  read(): T {
    return this.v;
  }
}

// The parent side is substituted at the `extends` args (`T` → `string`), so the
// unbound `U` belongs to the child and cannot be known to satisfy it. Compared
// as an opaque generic param — a bare `TypeVar` would match anything.
class UnboundOverride<U> extends GenericBase<string> {
  u: U;
  constructor(u: U) {
    super("parent");
    this.u = u;
  }
  read(): U {
    return this.u;
  }
}

function main(): void {}
