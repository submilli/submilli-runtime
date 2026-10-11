// A non-literal argument decides a type parameter over the object and array
// literals before it, as in tsc, and the literals fit what it decides.
interface Animal {
  name: string;
}

interface Dog extends Animal {
  bark: string;
}

function pick<T>(a: T, b: T): T {
  return b;
}

function three<T>(a: T, b: T, c: T): T {
  return c;
}

interface Point {
  x: number;
  y: number;
}

interface Tally {
  total: number;
  count: number;
}

function bump(acc: { total: number }, v: number): Tally {
  return { total: acc.total + v, count: 1 };
}

class Named {
  name: string = "c";
}

function fail(): never {
  throw new Error("fail");
}

function main(): void {
  const dog: Dog = { name: "d", bark: "w" };
  const animal: Animal = { name: "a" };
  assert(pick({ name: "z" }, animal).name === "a", "an object literal before a supertype");
  const xs: number[] = [4];
  assert(pick([1], xs)[0] === 4, "an array literal before an array");
  assert(three({ name: "x" }, { name: "y" }, animal).name === "a", "two literals");
  assert(pick({ name: "z", bark: "q" }, dog).bark === "w", "a literal that fits");
  assert(pick([{ name: "q" }], [dog]).length === 1, "two array literals still combine");

  // `null` only makes the binding nullable, and `never` adds nothing.
  const none: Animal | null = null;
  assert(pick({ name: "n" }, none) === null, "a `null` after a literal");
  assert(pick([1, 2], null) === null, "a `null` literal after an array literal");
  let caught = false;
  try {
    pick({ name: "f" }, fail());
  } catch (e) {
    caught = true;
  }
  assert(caught, "a `never` after a literal");
  // A class instance, which no literal fits, leaves the literal's binding.
  assert(pick({ name: "x" }, new Named()).name === "c", "a class instance after a literal");
  const named: Named[] = [new Named()];
  assert(pick([{ name: "x" }], named)[0].name === "c", "class instances in an array");
  const namedHolder: { a: Named } = { a: new Named() };
  assert(pick({ a: { name: "x" } }, namedHolder).a.name === "c", "a class instance in a field");

  // A literal whose type is a supertype of the later argument keeps the
  // binding, as tsc's common supertype does.
  const point: Point = { x: 1, y: 2 };
  assert(pick({ x: 0, y: null as number | null }, point).y === 2, "a wider literal");
  const totals = [1, 2, 3].reduce(
    (acc, x) => ({ sum: acc.sum + x, last: x }),
    { sum: 0, last: null as number | null },
  );
  assert(totals.sum === 6 && totals.last === 3, "a wider accumulator literal");
  // Only the literal itself rejects a property it lacks: a field holding a
  // declared type relates as usual.
  const holder: { d: Dog } = { d: dog };
  assert(pick({ d: animal }, holder).d.name === "d", "a declared type in a literal's field");
  assert(pick([animal], [dog])[0].name === "d", "a declared type in an array literal");
  // Several literals relate to a later argument as their union does.
  assert(three({ x: 1 }, { x: 1, y: 2 }, point).x === 1, "two literals before a subtype");
  // A callback's result is checked against what its parameter read, and
  // doesn't take the binding over.
  const counted = [1, 2, 3].reduce((acc, v) => bump(acc, v), { total: 0 });
  assert(counted.total === 6, "a callback returning a subtype");
}
