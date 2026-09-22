function main(): void {
  const t: [unknown, number, unknown, number | string, boolean | number] = [1, 2, true, 3, false];
  assert(t[0] === 1, "unknown number");
  assert(t[1] === 2, "number");
  assert(t[2] === true, "unknown boolean");
  assert(t[3] === 3, "union number");
  assert(t[4] === false, "union boolean");
  const nullable: [unknown, unknown, unknown] = [null, "text", { value: 4 }];
  assert(nullable[0] === null, "null");
  assert(nullable[1] === "text", "string");
}
