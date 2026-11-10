class Shape { size: number = 1 }
function keys<T>(key: keyof T): keyof T | undefined { return key; }
function maybeRead<T, K extends keyof T>(value: Partial<T>, key: K): T[K] | undefined { return value[key]; }
function anyKey<T, K extends keyof T>(value: T, key: K): T[keyof T] { return value[key]; }
function call<T>(make: () => T): T { const made: ReturnType<() => T> = make(); return made; }
function loosen<T>(value: Readonly<T>): Partial<T> { return value; }
function pick<T, K extends keyof T>(value: Pick<T, K>, key: K): K { return key; }
function sized<T>(value: Readonly<T>): number { if (value instanceof Shape) { return value.size; } return 0; }
function upper<T extends string, U extends T>(text: Uppercase<U>): Uppercase<T> { return text; }

function main(): void {
  const source = { a: 1, b: "s" };
  assert(keys<{ a: number }>("a") === "a" && maybeRead<{ a: number }, "a">({ a: 1 }, "a") === 1, "keyof and Partial<T>[K]");
  assert(anyKey({ a: 2 }, "a") === 2 && call(() => 3) === 3, "T[K] widens to T[keyof T], ReturnType<() => T> is T");
  assert(loosen<{ a: number }>({ a: 4 }).a === 4 && pick(source, "a") === "a", "Readonly<T> to Partial<T>, Pick inference");
  assert(sized(new Shape()) === 1 && upper<string, "A">("A") === "A", "instanceof and string mappings");
}
