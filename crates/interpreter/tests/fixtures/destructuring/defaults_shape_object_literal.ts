// An object literal an object pattern takes apart is typed from the pattern,
// as in tsc: a defaulted property the literal lacks reads `undefined` and
// takes its default, and one it has is optional in the literal's type, so the
// binding joins the property's type with the default's.
function size({ width = 10 } = { width: 1 }): number {
  return width;
}

function main(): void {
  const { a, b = a } = { a: 1 };
  assert(a === 1 && b === 1, "a default for a property the literal lacks");
  const { name, label = name + "!" } = { name: "hi", label: 2 };
  const joined: number | string = label;
  assert(name === "hi" && joined === 2, "a present property keeps its value");
  let { x = "a" } = {};
  x = "b";
  assert(x === "b", "a `let` pattern over an empty literal");
  const u = (({ u = 22 } = { u: 23 }) => u)();
  assert(u === 23 && size() === 1 && size({ width: 3 }) === 3, "parameter defaults");
}
