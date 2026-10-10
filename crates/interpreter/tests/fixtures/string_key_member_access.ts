// A string-literal key names a member exactly where `.name` would: a namespace
// constant, an enum member, and a field all read the same through either spelling.
enum Color {
  Red,
  Green,
}

function main(): void {
  assert(Math["PI"] === Math.PI, "a namespace constant");
  assert(Color["Green"] === Color.Green, "an enum member");
  const p = { x: 1 };
  assert(p["x"] === p.x, "a field");
}
