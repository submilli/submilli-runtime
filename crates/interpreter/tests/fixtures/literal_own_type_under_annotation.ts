// An array, tuple or object literal checked against an expected type keeps its
// own type, as in TypeScript, and is still checked against that type. Values
// built this way behave as the expected type wherever they are stored.

interface Point {
  at: number;
  label?: string;
}

type Result = { code: number; error?: string };

function describe(point: Point): string {
  return JSON.stringify(point);
}

function main(): void {
  const [first]: [string | number] = [1];
  const count: number = first;
  assert(count === 1, "a tuple literal keeps its element types");

  const words: (string | number)[] = ["a"];
  words.push(2);
  assert(words.length === 2 && words[1] === 2, "an array literal still takes the declared element type");

  const result: Result = { code: 10 };
  assert((result.error ?? "none") === "none", "an absent optional field reads as absent");
  result.error = "late";
  assert(result.error === "late", "an optional field left out can be set later");
  assert(JSON.stringify(result) === '{"code":10,"error":"late"}', "the set field serializes");

  const point: Point = { at: 1 };
  assert(describe(point) === '{"at":1}', "an absent optional field is omitted from JSON");
  point.label = "origin";
  assert(describe(point) === '{"at":1,"label":"origin"}', "an interface's optional field can be set later");

  let shape: Result = { code: 1, error: "e" };
  shape = { code: 2 };
  assert((shape.error ?? "none") === "none", "a write of a literal keeps the declared fields readable");
  shape.error = "again";
  assert(shape.error === "again", "and writable");
  console.log("ok");
}
