import { Base } from "@test/base";

class Sub extends Base {
  tag(): number {
    return this.count() + 1;
  }
}

function main(): void {
  const s = new Sub();
  assert(s.tag() === 1, "subclass inherits a dependency class's closure members");
}
