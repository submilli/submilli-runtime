// Spreading a union-typed method parameter. Codegen widens the parameter, and
// the spread still copies the fields its declared type names.
type Shape = { x: number } | { x: string; y: boolean };

class Copier {
  describe(u: Shape): string {
    const c = { ...u };
    return typeof c.x === "number" ? `number ${c.x}` : `string ${c.x}`;
  }
}

type Holder = { v: { a: number } | null };

function clear(h: Holder): void {
  h.v = null;
}

// The narrowing of `h.v` is stale once `clear` runs, so the spread must throw
// a catchable error, as a field read of it does, rather than trap.
function spreadStale(h: Holder): number {
  if (h.v !== null) {
    clear(h);
    try {
      const copy = { ...h.v };
      return copy.a;
    } catch (e) {
      return -1;
    }
  }
  return 0;
}

function main(): void {
  const copier = new Copier();
  assert(copier.describe({ x: 1 }) === "number 1", "number member");
  assert(copier.describe({ x: "a", y: true }) === "string a", "string member");
  assert(spreadStale({ v: { a: 1 } }) === -1, "a stale narrowed source throws a catchable error");
}
