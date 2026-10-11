// A return type with `void` in it needs no `return`, as in TypeScript.
function log(message: string): number | void {
  console.log(message);
}
function main(): void {
  log("x");
}
