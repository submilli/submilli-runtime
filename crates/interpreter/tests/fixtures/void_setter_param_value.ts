class Holder {
  called: boolean = false;
  set value(value: void) { this.called = value === undefined; }
}
function main(): void {
  const holder = new Holder();
  holder.value = undefined;
  assert(holder.called, "setter receives undefined in a void parameter");
}
