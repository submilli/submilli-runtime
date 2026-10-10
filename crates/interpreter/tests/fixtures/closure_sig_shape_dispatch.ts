// An interface method's dispatch sig is built at the call site from arity plus
// return type, so no type in scope names it: the receiver is an `InterfaceRef`
// (whose walk sees type arguments only) and the shape is never built here.
// Arity 7 so no other member or literal in the module supplies it.
interface Wide {
  go(a: number, b: number, c: number, d: number, e: number, f: number, g: number): number;
}

class Other {
  go(x: number): number {
    return x;
  }
}

function unused(w: Wide): number {
  return w.go(1, 2, 3, 4, 5, 6, 7);
}

function main(): void {
  assert(new Other().go(1) === 1, "class method supplies the shared field name");
}
