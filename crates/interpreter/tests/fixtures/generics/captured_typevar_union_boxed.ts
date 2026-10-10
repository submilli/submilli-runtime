// A captured-and-reassigned binding is stored in a box cell whose payload type
// is the erased `(ref null $Object)`. Reading one back must not assume the
// declared type is non-null: `T | U` names no `null`, but either parameter can
// be instantiated with a nullable type, so the cell legitimately holds null.
interface Named {
  name: string;
}

function pick<T, U>(x: T, y: U): string {
  let cell: T | U = y;
  const read = (): string => (cell === null ? "null" : "value");
  cell = x;
  return read();
}

function pickNamed<T>(x: T, y: Named): string {
  let cell: Named | T = y;
  const read = (): string => (cell === null ? "null" : "value");
  cell = x;
  return read();
}

function main(): void {
  assert(pick<string | null, number>(null, 1) === "null", "null through a T|U cell");
  assert(pick<string | null, number>("s", 1) === "value", "non-null through a T|U cell");
  assert(pickNamed<string | null>(null, { name: "n" }) === "null", "null through an interface|T cell");
  assert(
    pickNamed<string | null>("s", { name: "n" }) === "value",
    "non-null through an interface|T cell",
  );
}
