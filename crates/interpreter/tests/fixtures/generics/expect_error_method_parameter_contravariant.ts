// A method's parameter is contravariant, in an interface, a class and an
// object type alike, since nothing checks the argument when the method is
// called. tsc compares it both ways, which would let `h.f` read `bark` on a
// `Cat`.
// expect-error: expected `W<Animal>`, got `W<Dog>`
// expect-error: expected `{ f: (arg0: Animal) => string }`, got `{ f: (arg0: Dog) => string }`
// expect-error-count: 2
class Animal {
  name: string = "a";
}

class Dog extends Animal {
  bark: string = "woof";
}

class Cat extends Animal {
  meow: number = 3;
}

interface W<T> {
  f(x: T): string;
}

function main(): void {
  const o: W<Dog> = { f: (x: Dog) => x.bark + "!" };
  const h: W<Animal> = o;
  console.log(h.f(new Cat()));
  const p: { f(x: Dog): string } = { f: (x: Dog) => x.bark };
  const q: { f(x: Animal): string } = p;
}
