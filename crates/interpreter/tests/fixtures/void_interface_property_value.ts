interface HasVoid { value: void }
function main(): void {
  const object: HasVoid = { value: undefined };
  assert(object.value === undefined, "interface property accepts void");
}
