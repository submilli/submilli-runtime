interface A { b: Partial<B>; k: keyof B; v: B["x"] }
type View = Readonly<C>;
function patch(value: Partial<D>): number { return value.x ?? 0; }
interface B { x: number }
class C { y: number = 1 }
interface D { x: number }

function main(): void {
  const a: A = { b: { x: 1 }, k: "x", v: 3 };
  const view: View = new C();
  assert(a.b.x === 1 && a.k === "x" && a.v === 3, "an interface reads later declarations through utilities");
  assert(view.y === 1 && patch({ x: 4 }) === 4, "aliases and functions read later declarations");
}
