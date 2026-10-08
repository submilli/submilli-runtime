// An array literal expected as a union of array types is the first member
// whose element type every element fits; `[]` is the first member.
type Point = { x: number };
type Label = { text: string };

function pick(): number[] | string[] {
  return [];
}

function main(): void {
  const letters: ("a" | "b")[] | number[] = ["a", "b"];
  const numbers: string[] | number[] = [1, 2];
  const empty: number[] | string[] = [];
  const ro: readonly number[] | readonly string[] = ["z"];
  const spread: number[] | string[] = [...[1, 2], 3];
  const shapes: Point[] | Label[] = [{ text: "t" }];
  const callbacks: ((n: number) => number)[] | string[] = [(n) => n + 1];
  assert(letters.length === 2, "literal elements keep their member's type");
  assert(numbers[1] === 2, "the second member fits");
  assert(empty.length === 0 && pick().length === 0, "an empty literal fits");
  assert(ro[0] === "z", "a readonly member fits");
  assert(spread.length === 3, "a spread literal's elements choose the member");
  assert(shapes.length === 1, "object literals choose their member");
  assert(callbacks[0](1) === 2, "a callback takes its parameter type from the context");
}
