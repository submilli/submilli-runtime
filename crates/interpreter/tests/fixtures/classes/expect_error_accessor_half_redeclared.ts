// Redeclaring only one accessor half replaces the whole descriptor: reading a
// setter-only override, writing a getter-only override, or implementing a
// readable interface with a setter-only override are all rejected.
//
// expect-error: property `g` is write-only (no getter)
// expect-error: cannot assign to read-only accessor `n` on `GetterOnlySub`
// expect-error: member `note` is write-only (no getter)

interface Bag {
  note: string;
}

class Base {
  private v: string = "a";
  get g(): string {
    return this.v;
  }
  set g(x: string) {
    this.v = x;
  }
  get n(): number {
    return 1;
  }
  set n(_x: number) {}
  get note(): string {
    return this.v;
  }
  set note(x: string) {
    this.v = x;
  }
}

class SetterOnlySub extends Base {
  set g(_x: string) {}
}

class GetterOnlySub extends Base {
  get n(): number {
    return 2;
  }
}

class BadBag extends Base implements Bag {
  set note(_x: string) {}
}

export function main(): string {
  const s = new SetterOnlySub();
  const g = new GetterOnlySub();
  g.n = 3;
  return s.g;
}
