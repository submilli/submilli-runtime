// A local enum's name follows value scoping: a parameter, loop binding or
// nested function declared in an inner scope shadows it, and its members name
// literal types.

function shadows(): string {
  enum Kind { A = "a", B = "b" }
  const results: string[] = [];
  function inner(Kind: string): string {
    return Kind;
  }
  results.push(inner("param"));
  for (const Kind of ["loop"]) {
    results.push(Kind);
  }
  {
    function Kind(): string {
      return "fn";
    }
    results.push(Kind());
  }
  results.push(Kind.B);
  return results.join(",");
}

function tagged(n: number): string {
  enum Tag { Circle = "circle", Square = "square" }
  interface Circle { tag: Tag.Circle; r: number; }
  interface Square { tag: Tag.Square; side: number; }
  type Shape = Circle | Square;
  const shape: Shape = n > 0 ? { tag: Tag.Circle, r: n } : { tag: Tag.Square, side: 2 };
  const first: typeof Tag.Circle = Tag.Circle;
  if (shape.tag === Tag.Circle) {
    return `${first} ${shape.r}`;
  }
  return `square ${shape.side}`;
}

function main(): void {
  assert(shadows() === "param,loop,fn,b", "inner bindings shadow the enum");
  assert(tagged(3) === "circle 3", "enum members as literal types");
  assert(tagged(0) === "square 2", "narrowing on a local enum tag");
}
