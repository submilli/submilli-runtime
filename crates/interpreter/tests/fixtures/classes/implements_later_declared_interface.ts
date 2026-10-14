// `implements` conformance is checked once every class signature is bound, so a
// member the class inherits satisfies the interface wherever the interface is
// declared. (A parent class must come first: `extends` reads it at run time.)
class Base {
  name(): string {
    return "base";
  }
}

class Sub extends Base implements Named {
  id(): number {
    return 7;
  }
}

interface Named {
  name(): string;
}

function main(): void {
  const s = new Sub();
  assert(s.name() === "base");
  assert(s.id() === 7);

  // and through the interface it satisfies, which dispatches by payload scan
  const n: Named = s;
  assert(n.name() === "base");
}
