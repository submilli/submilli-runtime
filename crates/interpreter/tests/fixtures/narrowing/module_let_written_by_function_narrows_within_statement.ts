// A guard on a module-level `let` that a function assigns still narrows the
// code it guards.
let current: string | null = "a";

function clear(): void {
  current = null;
}

if (current !== null) {
  const text: string = current;
  console.log(text.length);
}
clear();

function main(): void {
  console.log(current === null);
}
