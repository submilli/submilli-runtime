function nothing(): void {}
class Holder { static value: void = nothing(); }
function main(): void {
  assert(Holder.value === undefined, "static void field stores undefined");
}
