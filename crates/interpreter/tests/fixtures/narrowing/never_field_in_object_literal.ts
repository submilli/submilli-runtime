// An object literal may hold a `never` value in code that never runs: inside
// a guard that ruled out every value, or as a call that can only throw.
function fail(): never {
  throw new Error("failed");
}

function label(): string {
  return "s";
}

function main(): void {
  const s = label();
  if (typeof s === "number") {
    const holder = { value: s };
    console.log("unreachable", holder);
  }
  try {
    const thrown = { value: fail() };
    console.log("unreachable", thrown);
  } catch (error) {
    console.log("caught");
  }
  console.log("done");
}
