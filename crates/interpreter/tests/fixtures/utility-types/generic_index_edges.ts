function read<T extends {}, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
function write<T extends {}, K extends keyof T>(value: T, key: K, next: T[K]): void { value[key] = next; }
function narrowed<T extends {}, K extends keyof T>(value: T | undefined, key: K): T[K] | undefined {
  if (value !== undefined) { return value[key]; }
  return undefined;
}
interface User { name: string; age: number }
function field<K extends keyof User>(user: User, key: K): User[K] { return user[key]; }
function setField<K extends keyof User>(user: User, key: K, next: User[K]): void { user[key] = next; }
function size<T extends [number, string]>(pair: T): T["length"] { return pair.length; }

function fails(f: () => unknown): string {
  try { f(); } catch (e) {
    if (e instanceof RangeError) { return "range"; }
    if (e instanceof TypeError) { return "type"; }
    if (e instanceof Error) { return e.name; }
  }
  return "none";
}

function main(): void {
  const xs = [1, 2, 3];
  assert(fails(() => read(xs, 5)) === "MissingValueError", "an index past the end is missing");
  assert(fails(() => read(xs, -1)) === "MissingValueError", "a negative index is missing");
  assert(fails(() => read("abc", 1.5)) === "MissingValueError", "a fractional index is missing");
  assert(fails(() => write(xs, -1, 0)) === "range" && fails(() => write(xs, 3, 0)) === "range", "writes stay in range");
  const bytes = new Uint8Array(2);
  write(bytes, 1, 300);
  assert(bytes[1] === 44 && fails(() => write(bytes, 2, 1)) === "range", "byte writes wrap and stay in range");
  const date = Temporal.PlainDate.from("2024-01-02");
  assert(read(date, "year") === 2024 && read(Temporal.Duration.from({ hours: 2 }), "hours") === 2, "Temporal getters");
  assert(fails(() => read(date, "add")) === "type", "Temporal methods are not read by name");
  assert(narrowed("xyz", 2) === "z" && narrowed([1, 2, 3], 2) === 3 && narrowed({ a: 1 }, "a") === 1, "narrowed receivers");
  const user: User = { name: "a", age: 3 };
  setField(user, "age", 4);
  assert(field(user, "name") === "a" && user.age === 4, "a concrete receiver with a generic key");
  const pair: [number, string] = [1, "x"];
  const two: 2 = size(pair);
  assert(two === 2, "a tuple's length is its literal");
}
