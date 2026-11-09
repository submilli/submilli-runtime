interface Parameters { x: number }
type Readonly = { y: string };
type Partial<T> = { wrapped: T };
enum Uppercase { A = "a" }
class Pick { v: number = 1 }
function main(): void {
  const p: Parameters = { x: 1 };
  const r: Readonly = { y: "s" };
  const w: Partial<number> = { wrapped: 2 };
  const u: Uppercase = Uppercase.A;
  const k = new Pick();
  const still: Required<{ a?: number }> = { a: 3 };
  assert(p.x === 1 && r.y === "s" && w.wrapped === 2 && u === Uppercase.A && k.v === 1 && still.a === 3, "module types shadow utility names");
}
