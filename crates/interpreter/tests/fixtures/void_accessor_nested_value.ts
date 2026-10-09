class Holder {
  count: number = 0;
  set values(values: void[]) { this.count = values.length; }
}
function main(): void {
  const holder = new Holder();
  holder.values = [undefined, undefined];
  assert(holder.count === 2, "accessor accepts an array of void values");
}
