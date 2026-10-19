// expect-error-count: 3
// expect-error: expected `Settable`, got `ReadonlyMap<string, number>`
// expect-error: expected `InheritsSet`, got `ReadonlyMap<string, number>`
// expect-error: cannot call `set` on `ReadonlyMap<string, number>`
// A readonly view does not satisfy an interface that requires one of the
// mutating methods it leaves out, inherited or not, while a mutable map does and
// an optional mutator is fine. A union with a readonly member refuses the
// mutators too. All as in tsc.
interface Settable {
  set(key: string, value: number): unknown;
}
interface InheritsSet extends Settable {
  readonly size: number;
}
interface MaybeSettable {
  get(key: string): number | undefined;
  set?(key: string, value: number): unknown;
}
function main(): void {
  const map: ReadonlyMap<string, number> = new Map<string, number>();
  const settable: Settable = map;
  const inherits: InheritsSet = map;
  const maybe: MaybeSettable = map;
  const mutable: Settable = new Map<string, number>();
  const either: ReadonlyMap<string, number> | Map<string, number> = new Map<string, number>();
  either.set("a", 1);
}
