type User = { id: number; name: string; email?: string };

function merge<T>(base: T, patch: Partial<T>): T { return base; }
function freeze<T>(value: T): Readonly<T> { return value; }
function thaw<T>(value: Readonly<T>): T { return value; }
function present<T>(value: T): NonNullable<T> {
  if (value === null || value === undefined) { throw new Error("missing"); }
  return value;
}
function orElse<T>(value: T, fallback: Exclude<T, null>): Exclude<T, null> { return fallback; }
function call<F extends (...args: never[]) => unknown>(f: F, fallback: ReturnType<F>): ReturnType<F> { return fallback; }
function pick<T, K extends keyof T>(value: T, key: K): Pick<T, K> { return value; }
function only<T>(change: Partial<T>): Partial<T> { return change; }
function withDefault<T>(value: T | undefined, fallback: NoInfer<T>): T { return value ?? fallback; }
function email<T extends { email?: string }>(change: Partial<T>): string { return change.email ?? "none"; }

class Box<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): NonNullable<T> | undefined {
    const value = this.value;
    return value === null || value === undefined ? undefined : value;
  }
}

function main(): void {
  const user: User = { id: 1, name: "ada" };
  assert(merge(user, { name: "grace" }).name === "ada", "Partial<T> beside T");
  const frozen = freeze<User>({ id: 2, name: "bo" });
  const thawed = thaw(frozen);
  thawed.name = "lin";
  assert(frozen.id === 2 && thawed.name === "lin", "Readonly<T> both ways");

  const flag: boolean = present<boolean | null>(true);
  const size: number = present(4 as number | undefined);
  const fallback: boolean = orElse<boolean | null>(null, false);
  const count: number = call(() => 3, 7);
  assert(flag && size + 1 === 5 && !fallback && count === 7, "primitive and boolean results");

  const picked = pick(user, "name");
  const change = only({ name: "z" });
  assert(picked.name === "ada" && change.name === "z", "object results and reverse inference");
  assert(withDefault<string>(undefined, "d") === "d" && email<User>({ email: "e" }) === "e", "NoInfer and reads through Partial<T>");

  const box = new Box<number | null>(3);
  assert(box.get() === 3, "NonNullable<T> in a class");
}
