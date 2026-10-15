// expect-error-count: 3
// expect-error: expected `Settable`, got `ReadonlyMap<string, number>`
// expect-error: expected `Pushable`, got `readonly number[]`
// expect-error: cannot call `set` on `ReadonlyMap<string, number>`
// A readonly view does not satisfy an interface that requires one of the
// mutating methods it leaves out, and a union with a readonly member refuses
// them too, as in tsc.
interface Settable {
  set(key: string, value: number): unknown;
}
interface Pushable {
  push(value: number): number;
}
function main(): void {
  const map: ReadonlyMap<string, number> = new Map<string, number>();
  const settable: Settable = map;
  const values: readonly number[] = [1];
  const pushable: Pushable = values;
  const either: ReadonlyMap<string, number> | Map<string, number> = new Map<string, number>();
  either.set("a", 1);
}
