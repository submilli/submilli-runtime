// `implements` conformance is checked once every class signature is bound, so a
// member the class inherits satisfies the interface regardless of where the
// parent is declared — this program compiles even though `Base` comes last.
interface Named {
  name(): string;
}

class Sub extends Base implements Named {
  id(): number {
    return 7;
  }
}

class Base {
  name(): string {
    return "base";
  }
}

function main(): void {
  const s = new Sub();
  assert(s.name() === "base");
  assert(s.id() === 7);

  // and through the interface it satisfies, which dispatches by payload scan
  const n: Named = s;
  assert(n.name() === "base");
}
