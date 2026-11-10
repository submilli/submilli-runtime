interface A { v: B["x"]; p: Partial<B>; k: keyof B; r: Readonly<Partial<B>>; o: Omit<B, "y"> }
interface B { x: number; y: string }
class User {
  name: string = "u";
  age: number = 1;
  patch(changes: Partial<User>): number { return changes.age ?? this.age; }
}
function excluded<T extends "a" | "b" | "c">(value: Exclude<T, "a">): "b" | "c" { return value; }
function extracted<T extends string | number>(value: Extract<T, string>): string { return value; }
function present<T extends string | null>(value: NonNullable<T>): string { return value; }

function main(): void {
  const parsed = JSON.parse('{"v": 1, "p": {"x": 2}, "k": "y", "r": {}, "o": {"x": 3}}') as A;
  const key: "x" | "y" = parsed.k;
  assert(parsed.v === 1 && parsed.p.x === 2 && key === "y" && parsed.o.x === 3, "a cast reads later declarations");
  let rejected = false;
  try { JSON.parse('{"v": 1, "p": {}, "k": "z", "r": {}, "o": {"x": 3}}') as A; } catch (e) { rejected = e instanceof TypeError; }
  assert(rejected, "a cast checks a later declaration's keys");
  assert(new User().patch({ age: 5 }) === 5, "a method takes a utility over its own class");
  assert(excluded<"a" | "b">("b") === "b" && extracted<string | number>("s") === "s" && present<string | null>("p") === "p", "utilities fit their operands' bounds");
}
