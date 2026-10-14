// A default that writes an earlier parameter may not run, so the body reads
// that parameter as what it held before the default or what the default left,
// without the `undefined` its own default replaced.
function viaClosure(
  a: number | string | undefined = 1,
  _ran: unknown = (() => {
    a = "";
  })()
): string {
  const read: number | string = a;
  return typeof read === "string" ? "text" : read.toFixed(0);
}

function direct(a: number | string | undefined = 1, b: string = (a = "set")): string {
  const read: number | string = a;
  return String(read) + b;
}

function clears(a: number | undefined = 1, _b: unknown = (a = undefined)): string {
  const read: number | undefined = a;
  return String(read);
}

function main(): void {
  assert(viaClosure() === "text" && viaClosure(2, 0) === "2", "a default's immediate closure");
  assert(direct() === "setset" && direct(3, "x") === "3x", "a default's direct write");
  assert(clears() === "undefined" && clears(4, 0) === "4", "a default that writes undefined");
}
