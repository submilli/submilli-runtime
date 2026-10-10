// An object literal is checked for unknown fields only where it is assigned
// straight to a declared type, as in tsc. A literal that a call infers a type
// parameter from, or that a function literal with no return annotation
// returns, is typed for itself first: its extra fields are kept, and it is
// related to the target structurally.

type A = { a: number };
interface Named {
  name: string;
}
type Shape = { kind: "c"; r: number } | { kind: "s"; w: number };

function id<T>(x: T): T {
  return x;
}

function both<T>(x: T, y: T): T {
  return y;
}

function orNull<T>(x: T | null): T | null {
  return x;
}

class Holder<T> {
  value: T;

  constructor(value: T) {
    this.value = value;
  }

  static of<T>(value: T): Holder<T> {
    return new Holder(value);
  }
}

type Weak = { a?: number; b?: string };

function boxedOrBare<T>(x: T | { v: T }, y: T): T {
  return y;
}

function make(f: () => A): A {
  return f();
}

function named(f: (n: number) => Named): Named {
  return f(1);
}

function main(): void {
  const viaId: A = id({ a: 1, z: 2 });
  assert(JSON.stringify(viaId) === "{\"a\":1,\"z\":2}", "a type argument's source keeps extra fields");

  const viaUnion: Shape = id({ kind: "c", r: 1, z: 2 });
  assert(viaUnion.kind === "c", "a union target still types the tag");
  assert(JSON.stringify(viaUnion) === "{\"kind\":\"c\",\"r\":1,\"z\":2}", "and keeps extra fields");

  const nested: { o: A } = id({ o: { a: 1, z: 2 } });
  assert(JSON.stringify(nested) === "{\"o\":{\"a\":1,\"z\":2}}", "so does a nested literal");

  const inArray: A[] = id([{ a: 1, z: 2 }]);
  assert(inArray.length === 1, "an array of literals is a source too");

  const second: A = both({ a: 1 }, { a: 2, z: 3 });
  assert(second.a === 2, "an argument after another that infers the same parameter");

  const constructed: Holder<A> = new Holder({ a: 1, z: 2 });
  assert(constructed.value.a === 1, "a generic class's constructor");

  const viaStatic: Holder<A> = Holder.of({ a: 2, z: 3 });
  assert(viaStatic.value.a === 2, "a generic method");

  const sharesAField: Weak = id({ a: 1, z: 2 });
  assert(sharesAField.a === 1, "a type whose fields are all optional, sharing one");

  const optionalA: Weak = { a: 1 };
  const spreadShares: Weak = id({ ...optionalA, z: 1 });
  assert(spreadShares.a === 1, "or sharing one through a spread");

  const otherMember: Weak | Named = id({ name: "n", z: 1 });
  assert(JSON.stringify(otherMember) === "{\"name\":\"n\",\"z\":1}", "or a union whose other member takes it");

  const insideStructure: A = boxedOrBare({ v: { a: 1, z: 2 } }, { a: 3, z: 4 });
  assert(insideStructure.a === 3, "a union parameter's member with structure around the parameter");

  const nullable: A | null = orNull({ a: 1, z: 2 });
  assert(nullable !== null && nullable.a === 1, "a nullable type parameter");

  const arrow = make(() => ({ a: 1, z: 2 }));
  assert(JSON.stringify(arrow) === "{\"a\":1,\"z\":2}", "an arrow's inferred return keeps extra fields");

  const block = make(() => {
    return { a: 2, z: 3 };
  });
  assert(block.a === 2, "so does a block body's return");

  const fromFunction = make(function () {
    return { a: 3, z: 4 };
  });
  assert(fromFunction.a === 3, "and a function expression's");

  const withParam = named((n) => ({ age: n, name: "n" }));
  assert(withParam.name === "n", "an interface target");

  const annotated: () => A = () => ({ a: 4, z: 5 });
  assert(annotated().a === 4, "a function-typed variable");

  const mapped: A[] = [1, 2].map((n) => ({ a: n, z: n }));
  assert(JSON.stringify(mapped) === "[{\"a\":1,\"z\":1},{\"a\":2,\"z\":2}]", "a callback to a generic method");
}
