// An array literal's element type is the type every element fits, which need
// not be the first element's, as tsc's best common type: a one-parameter
// function fits a two-parameter function type, so both are held as that.
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
class Dog extends Animal {
  bark(): string {
    return "woof";
  }
}

function main(): void {
  const fs = [(x: number) => x, (x: number, y: number) => x * y];
  assert(fs.map((f) => f(3, 4)).join(",") === "3,12", "two arities");

  const gs = [() => 7, (x: number) => x + 1, (x: number, y: number) => x - y];
  assert(gs.map((g) => g(5, 2)).join(",") === "7,6,3", "three arities, widest last");

  const hs = [(x: number, y: number) => x * y, (x: number) => x];
  assert(hs.map((h) => h(2, 5)).join(",") === "10,2", "widest first");

  const dog = new Dog("d");
  const animal = new Animal("a");
  const pets = [dog, animal];
  assert(pets.map((p) => p.name).join(",") === "d,a", "a base class instance after a subclass one");

  const full = { a: 1, b: 2 };
  const part: { a: number } = { a: 5 };
  const rows = [full, part];
  assert(rows.map((r) => String(r.a)).join(",") === "1,5", "a structural supertype");

  const n: number = 1;
  const maybe = nothing();
  const values = [n, maybe];
  assert(values.map((v) => (v === null ? "null" : String(v))).join(",") === "1,null", "a nullable variable");

  const one = (x: number) => x;
  const two = (x: number, y: number) => x * y;
  const held = [one, two];
  assert(held.map((f) => f(3, 4)).join(",") === "3,12", "functions held in variables");
  assert(firstOf([one, two])(2, 5) === 2, "under an unbound generic hint");
}

function nothing(): number | null {
  return null;
}

function firstOf<T>(items: T[]): T {
  return items[0];
}
