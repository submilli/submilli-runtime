// Closure shapes named only by class members, with no closure literal anywhere
// in the module. Each member's callback arity is unique and differs from every
// dispatch sig the class produces — a shared arity would let one member's sig
// cover another's and hide a missing walk. The bodies never mention the
// callbacks either, since referencing one makes it an expression whose type is
// collected by a different path.
class EventBus {
  private count: number = 0;

  subscribe(id: number, fn: (a: number, b: number, c: number, d: number, e: number, f: number) => number): void {
    this.count = this.count + id;
  }

  make(): (a: number, b: number, c: number) => number {
    throw new Error("never called");
  }

  get sink(): (a: string, b: string, c: string, d: string, e: string, f: string, g: string) => void {
    throw new Error("never read");
  }

  set relay(f: (a: number, b: number, c: number, d: number, e: number) => string) {
    this.count = this.count + 1;
  }

  seen(): number {
    return this.count;
  }
}

function main(): void {
  const bus = new EventBus();
  assert(bus.seen() === 0, "class with closure-typed members compiles");
}
