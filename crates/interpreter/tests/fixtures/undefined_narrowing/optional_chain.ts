function read(value: { name: string | null } | undefined | null): string {
  return value?.name ?? "missing";
}
function present(value: { name: string | null } | undefined): number {
  if (value?.name !== undefined) {
    if (value.name !== null) { return value.name.length; }
    return -1;
  }
  return 0;
}
function main(): void {
  assert(read(undefined) === "missing");
  assert(read(null) === "missing");
  assert(read({name: null}) === "missing");
  assert(read({name: "yes"}) === "yes");
  assert(present(undefined) === 0);
  assert(present({name: null}) === -1);
  assert(present({name: "yes"}) === 3);
}
