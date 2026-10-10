// expect-error: does not implement
// A getter whose type disagrees with the interface property fails conformance.
interface HasName {
  name: string;
}

class Wrong implements HasName {
  get name(): number {
    return 1;
  }
}

function main(): void {
  const h: HasName = new Wrong();
  assert(h.name === "x");
}
