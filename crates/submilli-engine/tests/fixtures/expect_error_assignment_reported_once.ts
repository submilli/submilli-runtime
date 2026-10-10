// A mismatched assignment is reported once, wherever it assigns.
// expect-error: parameter `a`: expected `number`, got `string`
// expect-error-count: 5
type Bin = (a: number, b: number) => number;
let moduleFn: Bin | null = null;
class Box {
  f: Bin = (a) => a;
}
interface Holder {
  f: Bin;
}
function main(): void {
  let local: Bin | null = null;
  local = (a: string) => 1;
  moduleFn = (a: string) => 1;
  const box = new Box();
  box.f = (a: string) => 1;
  const fs: Bin[] = [];
  fs[0] = (a: string) => 1;
  const holder: Holder = { f: (a) => a };
  holder.f = (a: string) => 1;
}
