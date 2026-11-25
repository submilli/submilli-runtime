function read<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
function write<T, K extends keyof T>(value: T, key: K, next: T[K]): void { value[key] = next; }
function maybe<T, K extends keyof T>(value: T | undefined, key: K): T[K] | undefined { return value?.[key]; }
class Account { owner: string = "o"; describe(): string { return this.owner; } }

function main(): void {
  const xs = [10, 20];
  const pair: [number, string] = [1, "s"];
  assert(read(xs, 1) === 20 && read(xs, "length") === 2 && read(pair, 1) === "s", "arrays and tuples");
  assert(read("abc", "length") === 3 && read("abc", 1) === "b", "strings");
  assert(read(new Map<string, number>([["a", 1]]), "size") === 1 && read(new Uint8Array(3), "length") === 3, "built-in values");
  write(xs, 0, 99);
  assert(xs[0] === 99 && maybe({ a: 1 }, "a") === 1, "array writes and optional reads");
  const account = new Account();
  write(account, "describe", () => "replaced");
  assert(
    account.describe() === "replaced" &&
      new Account().describe() === "o" &&
      Object.hasOwn(account, "describe") &&
      !Object.hasOwn(new Account(), "describe"),
    "writing a method key creates an own property on that instance",
  );
  assert(typeof read(xs, "push") === "function");
  let caught = 0;
  try { write(xs, 5, 1); } catch (e: RangeError) { caught += 1; }
  assert(caught === 1, "array writes stay in range");
}
