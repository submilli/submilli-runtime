// Comparing two `void` calls typechecks clean without this screen — there is
// no other diagnostic to abort compilation — and then codegen panics.
// expect-error: cannot compare `void`: an equality operand must be a value
function f(): void {}

function main(): void {
  const x = f() === f();
  const y = f() !== 1;
}
