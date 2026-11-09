// expect-error-count: 6
// expect-error: `in` operator requires a string or number on the left
function main(): void {
  const object = { a: 1 };
  const bool: boolean = true;
  const unknown: unknown = "a";
  bool in object;
  unknown in object;
  null in object;
  undefined in object;
  1n in object;
  ({}) in object;
}
