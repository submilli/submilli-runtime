function present(key: string | number, array: readonly number[]): boolean {
  return key in array;
}

function unionReceiver(value: number[] | { "0": string }): boolean {
  return 0 in value;
}

function unknownReceiver(value: unknown): boolean {
  if (typeof value === "object" && value !== null) return 0 in value;
  return false;
}

function main(): void {
  const array = [10, 20];
  assert(1 in array);
  assert(!(2 in array));
  assert(-0 in array);
  assert(!(1.5 in array));
  assert(!(-1 in array));
  assert(!(NaN in array));
  assert(!(Infinity in array));
  assert(present("0", array));
  assert(!present("-0", array));
  assert(!present("01", array));
  assert(present("length", array));
  assert(present("map", array));
  assert(!present("missing", array));
  assert(!present("\ud800", array));
  array.pop();
  assert(!(1 in array));
  array.push(30);
  assert(1 in array);
  const tuple: [number, string] = [1, "two"];
  assert(1 in tuple);
  assert(!(2 in tuple));
  const withUndefined: (number | undefined)[] = [undefined];
  assert(0 in withUndefined);
  const empty: number[] = [];
  assert(!(0 in empty));
  assert(unionReceiver(array));
  assert(unionReceiver({ "0": "zero" }));
  assert(unknownReceiver(array));
  assert(!unknownReceiver(1));
}
