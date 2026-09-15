// A closure capturing the catch binding (the boxed-catch path) sees the
// concrete Error value.
function main(): void {
  let describe: () => string = () => "none";
  try {
    throw new Error("captured");
  } catch (e) {
    describe = () => e.name + ": " + e.message;
  }
  assert(describe() === "Error: captured", "closure reads the captured catch binding");
}
