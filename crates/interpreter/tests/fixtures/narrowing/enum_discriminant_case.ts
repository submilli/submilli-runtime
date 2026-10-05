// A member that types its discriminant with an enum can hold the value of a
// `case` naming one of that enum's members, so the case keeps that member.
enum Kind {
  A = "x",
  B = "y",
}

type Shape = { kind: "a"; size: number } | { kind: Kind; label: string };

function describe(shape: Shape): string {
  switch (shape.kind) {
    case Kind.A: {
      const kept: Shape = shape;
      return kept.kind === Kind.A ? "enum A" : "other";
    }
  }
  return "default";
}

function main(): void {
  console.log(describe({ kind: Kind.A, label: "l" }), describe({ kind: "a", size: 1 }));
}
