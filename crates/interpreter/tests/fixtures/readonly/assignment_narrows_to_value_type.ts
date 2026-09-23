// A write narrows a binding to the value's own type, unless the declaration
// has something `readonly` for it to keep (see
// expect_error_readonly_survives_narrowing.ts). A subclass instance keeps its
// class, and an array its own element type.

class Base {
  hello(): string {
    return "base";
  }
}

class Sub extends Base {
  hello(): string {
    return "sub";
  }
}

class Animal {
  speak(): string {
    return "...";
  }
}

class Dog extends Animal {
  speak(): string {
    return "woof";
  }
}

function main(): void {
  let b: Base | Sub | null = null;
  b = new Sub();
  assert(b.hello() === "sub", "a union with a subclass member narrows to the value");

  let a: Animal | null = null;
  a = new Dog();
  let sawDog = false;
  if (a instanceof Dog) {
    sawDog = true;
  }
  assert(sawDog && a !== null && a.speak() === "woof", "narrowing survives an instanceof join");

  const pair: [number, number] = [1, 2];
  let list: number[] | [number, number] | null = null;
  list = pair;
  assert(list[0] + list.length === 3, "a tuple value keeps its tuple type");
}
