// A subclass satisfies its parent's `implements` contract through inherited
// members. Interface-typed dispatch scans the instance's object payload by
// name, so the subclass's instances must carry the inherited method closures
// too — the parent's declaration is not enough.
interface Container {
  get(): number;
  label(): string;
}

class Base implements Container {
  constructor(private n: number) {}
  get(): number {
    return this.n;
  }
  label(): string {
    return "base";
  }
}

class Kid extends Base {}

function read(c: Container): number {
  return c.get();
}

function tag(c: Container): string {
  return c.label();
}

function main(): void {
  const base = new Base(1);
  const kid = new Kid(2);

  // interface-typed parameter
  assert(read(base) === 1);
  assert(read(kid) === 2, "inherited method through an interface param");
  assert(tag(kid) === "base", "second inherited method");

  // interface-typed binding
  const c: Container = kid;
  assert(c.get() === 2, "inherited method through an interface binding");
  assert(c.label() === "base");

  // interface-typed array element
  const all: Container[] = [base, kid];
  assert(all[0].get() === 1);
  assert(all[1].get() === 2, "inherited method through an array element");
}
