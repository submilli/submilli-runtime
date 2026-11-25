// Inheriting an accessor without redeclaring it satisfies an interface;
// redeclaring only the setter replaces the prototype descriptor and clears the
// inherited getter.
interface Bag {
  note: string;
}

class Parent {
  private held: string = "initial";
  get note(): string {
    return this.held;
  }
  set note(value: string) {
    this.held = value;
  }
}

class InheritingChild extends Parent implements Bag {}

class SetterOnlyChild extends Parent {
  last: string = "";
  set note(value: string) {
    this.last = value;
  }
}

function main(): void {
  const bag: Bag = new InheritingChild();
  assert(bag.note === "initial", "unmodified inherited getter satisfies interface");

  const setterOnly = new SetterOnlyChild();
  const asParent: Parent = setterOnly;
  asParent.note = "updated";
  assert(setterOnly.last === "updated", "setter-only override handles writes");
  assert(
    (asParent.note as unknown) === undefined,
    "setter-only override clears the inherited getter",
  );
}
