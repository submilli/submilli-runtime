// A module-level object rest is a module variable, read from functions like any
// other; a `let` one can be reassigned.
const source = { id: 1, name: "one", active: true };
const { id, ...details } = source;
let { name, ...remaining } = source;

function describe(): string {
  return `${id} ${details.name} ${details.active} ${name} ${remaining.id}`;
}

function main(): void {
  assert(describe() === "1 one true one 1", "module rest read from a function");
  remaining = { id: 2, active: false };
  assert(remaining.id === 2 && describe() === "1 one true one 2", "a module let rest is reassignable");
}
