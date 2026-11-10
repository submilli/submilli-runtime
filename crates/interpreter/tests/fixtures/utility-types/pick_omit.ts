interface User { name: string; age?: number; readonly id: number; greet(): string }
type A = { kind: "a"; x: number; shared: string };
type B = { kind: "b"; y: number; shared?: string };

function main(): void {
  const picked: Pick<User, "name" | "age"> = { name: "p" };
  assert(picked.name === "p" && picked.age === undefined, "pick keeps optionality");
  const omitted: Omit<User, "greet" | "age"> = { name: "o", id: 1 };
  assert(omitted.id === 1, "omit");
  const unknownKey: Omit<User, "nope"> = { name: "n", id: 2, greet: () => "hi" };
  assert(unknownKey.greet() === "hi", "omit of an absent key keeps every field");

  const common: Pick<A | B, "kind" | "shared"> = { kind: "a" };
  assert(common.kind === "a" && common.shared === undefined, "pick over a union");
  const rest: Omit<A | B, "shared"> = { kind: "b" };
  assert(rest.kind === "b", "omit over a union keeps the common keys");
  const numeric: Pick<{ 0: string; 1: number }, 0> = { 0: "zero" };
  assert(numeric[0] === "zero", "numeric key");
}
