// A `never` value can be concatenated or interpolated, as in the message of
// an error thrown once every member was ruled out. That code never runs, and
// neither does what follows a call that can only throw.
function fail(): never {
  throw new Error("failed");
}

function describe(value: string | number): string {
  if (typeof value === "string") return "string";
  if (typeof value === "number") return "number";
  throw new Error(`unexpected ${value}: ` + value);
}

function concatenated(): string {
  return "a" + fail();
}

function interpolated(): string {
  return `a ${fail()} b`;
}

function main(): void {
  console.log(describe("s"), describe(1));
  try { console.log(concatenated()); } catch (error) { console.log("caught"); }
  try { console.log(interpolated()); } catch (error) { console.log("caught"); }
}
