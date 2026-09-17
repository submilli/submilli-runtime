type Read = { readonly x: number; readonly m(): number };
type Write = { x: number; m(): number };
function main(): void {
  const w: Write = { x: 2, m: (): number => 3 };
  const r: Read = { x: 4, m: (): number => 5 };
  assert(w.x + w.m() + r.x + r.m() === 14);
  const nested: { readonly child: { readonly x: number } } = { child: { x: 7 } };
  assert(nested.child.x === 7);
  const copy: Read = { ...r };
  assert(copy.x === 4 && copy.m() === 5);
}
