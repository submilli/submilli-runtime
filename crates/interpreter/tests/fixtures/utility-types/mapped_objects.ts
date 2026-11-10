interface User { name: string; age?: number; readonly id: number; nick?: string | undefined; note: string | undefined }
type Point = { x: number; y: number };
class Account { owner: string = "o"; balance?: number; describe(): string { return this.owner; } }

function main(): void {
  const partial: Partial<User> = {};
  assert(partial.name === undefined && partial.age === undefined, "partial interface");
  const partialPoint: Partial<Point> = { x: 1 };
  assert(partialPoint.x === 1 && partialPoint.y === undefined, "partial type literal");

  // `Required` strips the `undefined` of an optional field, even when spelled,
  // and keeps it on a required field.
  const required: Required<User> = { name: "n", age: 3, id: 1, nick: "k", note: undefined };
  const nick: string = required.nick;
  const age: number = required.age;
  assert(nick === "k" && age === 3 && required.note === undefined, "required");

  const frozen: Readonly<Point> = { x: 1, y: 2 };
  assert(frozen.x + frozen.y === 3, "readonly");

  const account: Partial<Account> = { owner: "a" };
  assert(account.owner === "a" && account.describe === undefined, "partial class instance");

  const counts: Partial<Record<string, number>> = { a: 1 };
  const missing: number | undefined = counts["b"];
  assert(counts["a"] === 1 && missing === undefined, "partial index signature");

  const twice: Required<Partial<User>> = { name: "n", age: 1, id: 2, nick: "k", note: "t" };
  assert(twice.age === 1, "required of partial");
}
