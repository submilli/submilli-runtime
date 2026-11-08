type User = { id: number; name: string };
type Shape = { kind: "square"; size: number } | { kind: "circle"; radius: number };
type Value<T, Key extends keyof T> = T[Key];
type Shout<S extends string> = Uppercase<S>;
type Args<F extends (...args: never[]) => unknown> = Parameters<F>;
type ByKind<K extends Shape["kind"]> = Extract<Shape, { kind: K }>;
type Patch<T> = Partial<T>;
interface Entry<T> { key: keyof T; value: T[keyof T]; frozen: Readonly<T> }

function main(): void {
  const id: Value<User, "id"> = 4;
  const loud: Shout<"ab"> = "AB";
  const args: Args<(a: number) => void> = [1];
  const square: ByKind<"square"> = { kind: "square", size: 4 };
  const patch: Patch<User> = { name: "n" };
  const entry: Entry<{ a: number }> = { key: "a", value: 1, frozen: { a: 2 } };
  const key: "a" = entry.key;
  assert(id === 4 && loud === "AB" && args[0] === 1 && square.size === 4, "generic aliases over utilities");
  assert(patch.name === "n" && key === "a" && entry.frozen.a === 2, "generic members evaluate at their arguments");
}
