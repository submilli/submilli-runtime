class Account { owner: string = "o"; balance: number = 5; describe(): string { return this.owner; } }

function read<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
function keysOf<T extends object>(value: T, first: keyof T): (keyof T)[] { return [first]; }
function label<T extends { name: string }, K extends keyof T>(value: T, key: K): string {
  return value.name + "=" + String(value[key]);
}

function main(): void {
  const name: string = read({ id: 1, name: "ada" }, "name");
  const balance: number = read(new Account(), "balance");
  const zero: string = read({ 0: "zero" }, 0);
  assert(name === "ada" && balance === 5 && zero === "zero", "T[K] reads");
  assert(keysOf({ a: 1, b: 2 }, "b")[0] === "b", "keyof T keeps the literal");
  assert(label({ name: "n", size: 2 }, "size") === "n=2", "T[K] in a body");
}
