// A type parameter several arguments give candidates for is inferred as their
// common supertype, as in tsc, rather than the first argument's type: a later
// argument of a wider type widens it. Object and array literals combine with
// each other, `null` is added back to the supertype of the rest, and a `never`
// argument, which fits any type, leaves the type to the others.
interface Animal {
  name: string;
}

interface Dog extends Animal {
  bark: string;
}

interface Box<T> {
  v: T;
}

function pick<T>(a: T, b: T): T {
  return b;
}

function pickBox<T>(b: Box<T>, d: T): T {
  return d;
}

function fail(): never {
  throw new Error("never returns");
}

function main(): void {
  const dog: Dog = { name: "d", bark: "w" };
  const a: Animal = { name: "a" };
  const boxed: Box<Dog> = { v: dog };

  assert(pick(dog, a).name === "a", "a later wider argument widens the binding");
  assert(pick(a, dog).name === "d", "a later narrower argument fits it");
  assert(pickBox(boxed, a).name === "a", "nested in a generic interface");
  assert(pickBox({ v: dog }, a).name === "a", "nested in an object literal");
  assert(pick({ v: dog }, { v: a }).v.name === "a", "two object literals combine");
  assert(pick([dog], [a])[0].name === "a", "two array literals combine");
  assert(pick([1], [null]).length === 1, "an array of nulls joins an array of numbers");

  const maybe = pick(1, null);
  assert(maybe === null, "null is added back to the other candidates");

  const fns = [() => fail(), () => 1];
  assert(fns[1]() === 1, "a closure returning never leaves the element type to the others");
  let caught = false;
  try {
    pick(fail(), "q");
  } catch (e) {
    caught = true;
  }
  assert(caught, "a never argument leaves the type to the other argument");
}
