// A typo in a function type's parameter list makes the parens look like a grouped
// type. The parse recovers so the diagnostic still names the parameter rule rather
// than reporting a stray `)`.
// expect-error: expected `:` after parameter name
// expect-error: function-type params require named annotations
type Bad = (x number) => string;

function main(): void {
  console.log("unreachable");
}
