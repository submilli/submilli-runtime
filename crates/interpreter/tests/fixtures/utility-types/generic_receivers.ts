// Generic reads and writes through bounded receivers, mapped utilities and
// static methods over their own class.
function maybe<T extends { a: number }, K extends keyof T>(value: T | undefined, key: K): T[K] | undefined { return value?.[key]; }
function anyObject<T extends object, K extends keyof T>(value: T | null, key: K): T[K] | undefined { return value?.[key]; }
function picked<T extends {}, K extends keyof T>(value: Pick<T, K>, key: K): T[K] { return value[key]; }
function pickedKey<T extends {}, K extends keyof T>(key: K): keyof Pick<T, K> { return key; }
function count<T extends {}, K extends keyof T>(value: T, keys: K[]): number { return keys.length; }
function total<R extends Record<string, number>>(record: R, key: string): number { return (record[key] ?? 0) + 1; }
function setA<T extends { a: number }>(value: T): void { value["a"] = 5; }
function patchA<T extends { a: number }>(patch: Partial<T>): void { patch["a"] = 6; }
function held<T, K extends keyof T>(value: T, key: K): T[K] { const read = value[key]; return read; }
function label<T, K extends Extract<keyof T, string>>(value: T, key: K): string { return `${key}:${String(value[key])}`; }
function rest<T, K extends keyof T>(value: T, key: Exclude<keyof T, K>): T[Exclude<keyof T, K>] { return value[key]; }
class Box {
  value: string = "v";
  static key(key: keyof Box): string { return key; }
  static read(box: Partial<Box>): string { return box.value ?? "none"; }
  static typed(value: Box["value"]): string { return value; }
}
interface User { name: string; age: number }

function main(): void {
  const user: User = { name: "u", age: 2 };
  assert(maybe({ a: 1 }, "a") === 1 && maybe(undefined, "a") === undefined && anyObject(user, "age") === 2, "optional chains on bounded receivers");
  assert(picked<User, "name">({ name: "p" }, "name") === "p" && pickedKey<User, "age">("age") === "age", "Pick reads and keys");
  assert(count(user, ["name", "age"]) === 2, "key arrays infer their literal keys");
  assert(total({ a: 1 }, "a") === 2 && total({ a: 1 }, "b") === 1, "a record bound reads its element type");
  const target = { a: 1 };
  setA(target);
  const patch: Partial<{ a: number }> = {};
  patchA(patch);
  assert(target.a === 5 && patch.a === 6, "writes through a bound and a partial");
  assert(Box.key("value") === "value" && Box.read({}) === "none" && Box.typed("x") === "x", "static methods over their own class");
  const optional: { a?: number } = {};
  assert(held(optional, "a") === undefined, "a local holds an absent optional property as undefined");
  assert(label(user, "name") === "name:u" && rest<User, "age">(user, "name") === "u", "keys bounded by utilities over keyof");
}
