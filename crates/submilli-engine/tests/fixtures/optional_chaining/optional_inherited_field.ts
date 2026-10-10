class Base {
  constructor(public tag: string) {}
}

class Derived extends Base {
  constructor(
    tag: string,
    public extra: number,
  ) {
    super(tag);
  }
}

function tag_of(d: Derived | null): string | undefined {
  return d?.tag;
}

function main(): void {
  const d = new Derived("base-field", 3);
  assert(tag_of(d) === "base-field", "field declared on the parent");
  assert(tag_of(null) === undefined, "short-circuit");
  assert(d?.extra === 3, "field declared on the subclass");
}
