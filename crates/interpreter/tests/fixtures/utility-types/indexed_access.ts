interface User { name: string; age?: number; tags: string[]; greet(): string }
type A = { kind: "a"; x: number };
type B = { kind: "b"; x: string };
class Account { owner: string = "o"; private secret: number = 1; balance(): number { return this.secret; } }
function songs(): { id: number; title: string }[] { return [{ id: 1, title: "one" }]; }

function main(): void {
  const name: User["name"] = "n";
  const age: User["age"] = undefined;
  const either: User["name" | "age"] = 3;
  const tag: User["tags"][number] = "t";
  const song: ReturnType<typeof songs>[number] = songs()[0];
  assert(name === "n" && age === undefined && either === 3 && tag === "t" && song.title === "one", "property types");

  const kind: (A | B)["kind"] = "b";
  const x: (A | B)["x"] = "s";
  const optional: [number, string?][1] = undefined;
  const element: [number, string][number] = 1;
  const value: Record<string, boolean>[string] = true;
  const owner: Account["owner"] = "o";
  const any: User[keyof User] = "anything";
  const size: number[]["length"] = 2;
  assert(kind === "b" && x === "s" && optional === undefined && element === 1, "unions and tuples");
  assert(value && owner === "o" && any === "anything" && size === 2, "records, classes and keyof");

  const common: keyof (A | B) = "x";
  const members: keyof Account = "balance";
  assert(common === "x" && members === "balance", "keyof of unions and class instances");
}
