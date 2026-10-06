// A generic call takes its type parameters from the type its result is
// expected to have, before its arguments are inferred, so a callback argument
// sees them. Where that type is a union, such as an optional comparator
// `Cmp<P> | null`, the result is matched against the members of its own kind.
// Where another member of that kind doesn't match, the arguments decide alone:
// the result may still be assigned to it. tsc accepts each of these.

type Cmp<T> = (a: T, b: T) => number;

function by<T>(key: (x: T) => number): Cmp<T> {
  return (a, b) => key(a) - key(b);
}

function wrap<T>(f: (x: T) => number): { apply: (x: T) => number } {
  return { apply: f };
}

function box<T>(value: T): { value: T } {
  return { value };
}

interface Person {
  age: number;
}

interface Labeled {
  value: string;
}

type Letter = "a" | "b";

function compareOrZero(cmp: Cmp<Person> | null, a: Person, b: Person): number {
  return cmp === null ? 0 : cmp(a, b);
}

function main(): void {
  const people: Person[] = [{ age: 3 }, { age: 1 }, { age: 2 }];
  const sorted = people.toSorted(by((p) => p.age));
  assert(sorted.map((p) => p.age).join(",") === "1,2,3", "toSorted's optional comparator");

  people.sort(by((p) => -p.age));
  assert(people.map((p) => p.age).join(",") === "3,2,1", "sort's optional comparator");

  assert(compareOrZero(by((p) => p.age), { age: 1 }, { age: 4 }) === -3, "a nullable parameter");

  const declared: Cmp<Person> | null = by((p) => p.age * 2);
  assert(declared !== null && declared({ age: 1 }, { age: 2 }) === -2, "a nullable declaration");

  const wrapped: { apply: (x: Person) => number } | null = wrap((p) => p.age + 1);
  assert(wrapped !== null && wrapped.apply({ age: 4 }) === 5, "an object member of the union");

  const either: Cmp<Person> | ((a: Person) => number) = by((p) => p.age);
  assert(either({ age: 5 }, { age: 2 }) === 3, "two members that agree");

  const letters: Letter[] | null = [1, 2].map((n) => (n > 1 ? "a" : "b"));
  assert(letters !== null && letters.join("") === "ba", "a generic method's result");

  const labeled: { value: number } | Labeled = box("s");
  assert(JSON.stringify(labeled) === "{\"value\":\"s\"}", "the argument decides past another object member");
}
