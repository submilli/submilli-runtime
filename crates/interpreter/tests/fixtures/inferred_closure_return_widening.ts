// An unannotated closure's return type is the widest of its `return`s, in
// whichever order they appear: every returned value has to fit it.

class Animal {
  legs: number = 4;
}
class Bird extends Animal {
  wings: number = 2;
}

class Fish extends Animal {
  fins: number = 3;
}

function boom(): never {
  throw new Error("boom");
}

function lookup(words: string[], index: number): string | null {
  return index < words.length ? words[index] : null;
}

function main(): void {
  const words: string[] = ["a", "v"];
  const key: string = words[0];

  // A subclass returned before its base: the closure returns the base.
  const narrowFirst = () => {
    if (key === "b") {
      return new Bird();
    }
    return new Animal();
  };
  assert(narrowFirst().legs === 4, "base returned through a closure typed by its subclass first");

  const wideFirst = () => {
    if (key === "b") {
      return new Animal();
    }
    return new Bird();
  };
  assert(wideFirst().legs === 4, "subclass returned as its base");

  // `null` before a nullable value: the closure is nullable, not `null`.
  const nullFirst = () => {
    if (key === "b") {
      return null;
    }
    return lookup(words, 1);
  };
  const found = nullFirst();
  assert(found === "v", "the nullable return keeps its value");

  // A diverging return does not name the type, wherever it sits.
  const neverFirst = () => {
    if (key === "z") {
      return boom();
    }
    if (key === "b") {
      return new Bird();
    }
    return new Animal();
  };
  assert(neverFirst().legs === 4, "diverging return before the others");

  // Siblings returned before their common base, which arrives last.
  const baseLast = () => {
    if (key === "b") {
      return new Bird();
    }
    if (key === "c") {
      return new Fish();
    }
    return new Animal();
  };
  assert(baseLast().legs === 4, "the base names the type wherever it sits");
}
