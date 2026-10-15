// A write through an interface that views a built-in value goes to the
// built-in's own setter (`length` truncates an array, `lastIndex` moves a
// regex), and a write to a read-only built-in property throws a catchable
// `TypeError` instead of trapping.
interface HasLength {
  length: number;
}
interface HasLastIndex {
  lastIndex: number;
}
interface HasSize {
  size: number;
}
function truncate(view: HasLength, length: number): void {
  view.length = length;
}
function main(): void {
  const xs = [1, 2, 3, 4];
  truncate(xs, 2);
  assert(xs.length === 2 && xs.join(",") === "1,2");
  const re = /a/g;
  const cursor: HasLastIndex = re;
  cursor.lastIndex = 3;
  assert(re.lastIndex === 3);
  const sizes: HasSize = new Map<string, number>();
  let caught = false;
  try {
    sizes.size = 3;
  } catch (error) {
    caught = error instanceof TypeError;
  }
  assert(caught);
  console.log("ok");
}
