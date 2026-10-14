function copyUnion(source: { a: number } | { b: string }): void {
  const result = Object.assign({}, source);
  if ("a" in result) { const value: number = result.a; assert(value === 1); }
}
function copyUnknown(source: unknown): number {
  return Object.assign({ a: 1 }, source).a;
}
class Destination {
  private value: number = 0;
  set count(value: number) { this.value = value; }
  get count(): number { return this.value; }
}
function main(): void {
  copyUnion({ a: 1 });
  assert(copyUnknown({ b: 2 }) === 1);
  const target = { first: 1 };
  const result = Object.assign(target, { second: "two" }, { third: true });
  assert(result.first === 1);
  const text: string = result.second;
  assert(text === "two");
  assert(result.third);
  result.first = 9;
  assert(target.first === 9);
  const destination = new Destination();
  const copied = Object.assign(destination, { extra: "new", count: 8 });
  assert(destination.count === 8);
  assert(copied.extra === "new");
  const restored: Destination = copied;
  assert(restored.count === 8);
  const array = [7, 8, 9];
  const assigned = Object.assign(array, [1]);
  assert(assigned.join(",") === "1,8,9");
  Object.assign(array, { length: 1 });
  assert(array.length === 1 && array[0] === 1);
  const higher: Record<string, number> = { "2": 4 };
  Object.assign(array, higher);
  assert(array.length === 3 && array[1] === undefined && array[2] === 4);
  const dictionary: Record<string, unknown> = {};
  const lone = String.fromCharCode(55296);
  const source: Record<string, string> = { [lone]: lone };
  Object.assign(dictionary, source);
  assert(dictionary[lone] === lone);
  const grouped = Object.groupBy([1, 2, 3], (value: number): string => value % 2 === 0 ? "even" : "odd");
  const odd: number[] | undefined = grouped.odd;
  assert(odd !== undefined && odd.length === 2 && odd[0] === 1 && odd[1] === 3);
  assert(grouped.even![0] === 2);
  assert(grouped.missing === undefined);
  const lengths = Object.groupBy(["hello", "test", "world"], (value: string): number => value.length);
  assert(Object.keys(lengths).join(",") === "4,5");
  const chars = Object.groupBy("🥰💩🙏😈", (value: string): string => value < "🙏" ? "before" : "after");
  assert(chars.before!.join("") === "💩😈");
  assert(chars.after!.join("") === "🥰🙏");
  const implicit = Object.groupBy("aba", char => char);
  assert(implicit.a!.length === 2);
  const live = [1];
  const appended = Object.groupBy(live, (value: number, index: number): string => {
    if (index === 0) live.push(2);
    return "items";
  });
  assert(appended.items!.join(",") === "1,2");

  const orderedSource: Record<string, number> = { ["length"]: 1, ["2"]: 4 };
  const orderedTarget: number[] = [];
  Object.assign(orderedTarget, orderedSource);
  assert(orderedTarget.length === 1);

}
