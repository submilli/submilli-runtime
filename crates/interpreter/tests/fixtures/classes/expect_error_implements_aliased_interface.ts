// expect-error: class `P` does not implement `N2`: missing member `name`
// expect-error: class `Q` does not implement `N2`: missing member `greet`
// Conformance is checked through the alias, exactly as it is when the interface
// is named directly — an aliased target must not skip the check.
interface Named {
  name: string;
  greet(): string;
}

type N2 = Named;

class P implements N2 {
  greet(): string {
    return "hi";
  }
}

class Q implements N2 {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}

function main(): void {}
