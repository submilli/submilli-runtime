// A method compares its parameters bivariantly, as in TypeScript, and a generic
// class that only mentions its parameter in a reference to itself doesn't read
// it, so these comparisons can be true and are accepted.
class Base { b: number = 1; }
class Derived extends Base { d: number = 1; }
function literals(p: { g(x: Derived): void; h(x: Base): void },
                  q: { g(x: Base): void; h(x: Derived): void }): boolean { return p === q; }
interface MP { g(x: Derived): void; h(x: Base): void }
interface MQ { g(x: Base): void; h(x: Derived): void }
function interfaces(p: MP, q: MQ): boolean { return p === q; }
class CP { g(x: Derived): void {} h(x: Base): void {} }
class CQ { g(x: Base): void {} h(x: Derived): void {} }
function classes(p: CP, q: CQ): boolean { return p === q; }
class K<T> { v: number = 1; next: K<T> | null = null; }
function selfReference(a: K<string>, b: K<number>): boolean { return a === b; }
function main(): void {
  assert(!literals({ g: (x: Derived): void => {}, h: (x: Base): void => {} },
                   { g: (x: Base): void => {}, h: (x: Derived): void => {} }), "distinct closures");
  assert(!classes(new CP(), new CQ()), "distinct classes");
  assert(selfReference(new K<string>(), new K<number>()) === (new K<string>() === new K<number>()),
    "a self-referencing class compares like any other");
}
