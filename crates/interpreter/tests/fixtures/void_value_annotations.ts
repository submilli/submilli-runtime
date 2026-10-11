function nothing(): void {}
function identity<T>(value: T): T { return value; }
interface Cell<T> { value: T }
function main(): void {
  const value: void = undefined;
  const values: void[] = [value, nothing()];
  const pair: [number, void] = [1, nothing()];
  const cell: Cell<void> = { value: identity<void>(nothing()) };
  const consume: (value: void) => number = (value: void): number => value === undefined ? 1 : 0;
  assert(values[0] === undefined, "void array stores undefined");
  assert(pair[1] === undefined, "void tuple position stores undefined");
  assert(cell.value === undefined, "void generic object field stores undefined");
  assert(consume(nothing()) === 1, "void parameter receives undefined");
  const unknownValue: unknown = nothing();
  assert(unknownValue === undefined, "void call can be used as unknown");
}
