class Account { owner: string = "o"; balance: number = 5; describe(): string { return this.owner; } }

function read<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
function write<T, K extends keyof T>(value: T, key: K, next: T[K]): void { value[key] = next; }
function copy<T, U extends T, K extends keyof T>(target: T, source: U, key: K): void { target[key] = source[key]; }
function keysOf<T extends object>(value: T, first: keyof T): (keyof T)[] { return [first]; }
function label<T extends { name: string }, K extends keyof T>(value: T, key: K): string {
  return value.name + "=" + String(value[key]);
}

function main(): void {
  const name: string = read({ id: 1, name: "ada" }, "name");
  const balance: number = read(new Account(), "balance");
  const zero: string = read({ zero: "zero" }, "zero");
  assert(name === "ada" && balance === 5 && zero === "zero", "T[K] reads");
  assert(keysOf({ a: 1, b: 2 }, "b")[0] === "b", "keyof T keeps the literal");
  const user = { id: 1, name: "ada" };
  write(user, "name", "grace");
  const account = new Account();
  write(account, "balance", 9);
  copy(user, { id: 7, name: "lin" }, "id");
  assert(user.name === "grace" && account.balance === 9 && user.id === 7, "T[K] writes");
  assert(label({ name: "n", size: 2 }, "size") === "n=2", "T[K] in a body");
}
