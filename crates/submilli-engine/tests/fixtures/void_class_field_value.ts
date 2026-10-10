class Holder { value: void = undefined; }
function main(): void {
  const holder = new Holder();
  assert(holder.value === undefined, "void field initializes to undefined");
  holder.value = undefined;
  assert(holder.value === undefined, "void field accepts undefined write");
}
