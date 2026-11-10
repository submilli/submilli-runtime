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
function named<K extends { a: number }>(value: Pick<K, "a">): number { return value.a; }
function omitted<T extends { a: number; b: string }>(value: Omit<T, "a">): void { value["b"] = "y"; }
type Handler<K extends string> = `on${Capitalize<K>}`;
function first<T, V extends T[keyof T]>(value: T, item: V): V { return item; }
function strict<R extends Record<string, number>, K extends keyof R>(value: Pick<R, K>, key: K): R[K] { return value[key]; }
function on<K extends string>(event: K, handler: `on${Capitalize<K>}`): string { return handler; }
type Select<T, K extends keyof T> = Pick<T, K>;
type Swap<K, T extends keyof K> = Select<K, T>;
function kinds<T extends { kind: "a" | "b" }, V extends T["kind"]>(value: T, kind: V): V[] { return [kind]; }
function strings<U extends string | number, K extends Extract<U, string>>(value: U, key: K): K[] { return [key]; }
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
  const target2 = { a: 1, b: "x" };
  omitted(target2);
  const handler: Handler<"click"> = "onClick";
  let widened = first({ a: "x" as string }, "x");
  widened = "y";
  assert(named({ a: 3 }) === 3 && target2.b === "y" && handler === "onClick" && widened === "y", "user type parameters named like Pick's, Omit writes, mapped template holes, widened values");
  let missing = false;
  try { strict<Record<string, number>, string>({}, "zz"); } catch (e) { missing = e instanceof Error && e.name === "MissingValueError"; }
  assert(missing, "a Pick over a record bound keeps missing reads");
  const swapped: Swap<User, "age"> = { age: 4 };
  const literalKinds: "a"[] = kinds({ kind: "a" }, "a");
  const literalKeys: "s"[] = strings("s", "s");
  assert(on("click", "onClick") === "onClick" && swapped.age === 4 && literalKinds[0] === "a" && literalKeys[0] === "s", "instantiated template holes, aliases over aliases, literals against utility bounds");
}
