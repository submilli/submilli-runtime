// Spreading a union-typed method parameter. Codegen widens the parameter, and
// the spread still copies the fields its declared type names.
type Shape = { x: number } | { x: string; y: boolean };

class Copier {
  describe(u: Shape): string {
    const c = { ...u };
    return typeof c.x === "number" ? `number ${c.x}` : `string ${c.x}`;
  }
}

function main(): void {
  const copier = new Copier();
  assert(copier.describe({ x: 1 }) === "number 1", "number member");
  assert(copier.describe({ x: "a", y: true }) === "string a", "string member");
}
