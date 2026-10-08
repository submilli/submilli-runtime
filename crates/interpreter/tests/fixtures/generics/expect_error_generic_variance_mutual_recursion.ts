// An interface used contravariantly through another interface that names it
// back is invariant, as tsc measures it: `A<Dog>` is not an `A<Animal>`.
// expect-error: expected `A<Animal>`, got `A<Dog>`
// expect-error-count: 1
interface Animal {
  name: string;
}

interface Dog extends Animal {
  bark: string;
}

interface A<Y> {
  g: B<Y>;
  h: Y;
}

interface B<X> {
  f: (a: A<X>) => void;
  k: X;
}

function widen(a: A<Dog>): A<Animal> {
  return a;
}

function main(): void {}
