type Missing = typeof undefined;
function shadowedBinding(): string {
  const undefined: string = "shadow";
  const shadowed: typeof undefined = undefined;
  return shadowed;
}
function main(): void {
  const missing: Missing = undefined;
  assert(missing === undefined, "typeof undefined resolves the built-in value binding");
  assert(shadowedBinding() === "shadow", "local bindings shadow the built-in in type queries");
}
