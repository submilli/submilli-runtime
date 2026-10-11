// An unannotated `[]` holds no element, so it is `never[]`, as in tsc: it can be
// read, iterated, spread and nested, and fits any array type it meets later. An
// element another type filled it with is a real value: every operation tsc
// accepts on `never` runs on it as it would in JavaScript.
function fill(list: number[]): void {
  list.push(41);
}

function flag(list: boolean[]): void {
  list.push(true);
}

function main(): void {
  let visits = 0;
  for (const v of []) {
    visits += 1;
  }
  assert(visits === 0, "a for-of over [] runs no iteration");

  const empty = [];
  assert(empty.length === 0, "a const [] reads its length");

  const spread = [...[], 1, ...[]];
  assert(spread.length === 1 && spread[0] === 1, "spreading [] adds nothing");

  const nested = [[], [2]];
  const trailing = [[3], []];
  const onlyEmpty = [[], []];
  assert(JSON.stringify([nested, trailing]) === "[[[],[2]],[[3],[]]]", "[] joins a sibling's array type");
  assert(onlyEmpty.length === 2, "arrays of empty arrays");

  const box = { items: [] };
  const make = () => [];
  assert(box.items.length === 0 && make().length === 0, "[] in a field or returned");
  assert([].join("-") === "" && `${[]}` === "", "[] joins to an empty string");

  const filled = { list: [] };
  fill(filled.list);
  assert(filled.list.length === 1, "a never[] another type fills holds what it was given");
  const first = filled.list[0];
  assert(first === 41, "reading an element it was given");
  const seen: number[] = [];
  for (const item of filled.list) {
    seen.push(item);
  }
  assert(seen.join(",") === "41", "iterating the elements it was given");
  const shown = `${filled.list[0]}`;
  assert(shown.length === 2 && shown === "41", "interpolating an element it was given");
  assert(`${filled.list[0]}${filled.list[0]}` === "4141", "interpolations concatenate as strings");
  const flags = { list: [] };
  flag(flags.list);
  assert(`is ${flags.list[0]}${1}` === "is true1", "interpolating a boolean beside text and a number");

  let counted = 0;
  for (let v of []) {
    v++;
    counted = v;
  }
  assert(counted === 0, "`++` on an element of [] type-checks; the loop never runs");

  let total = 1;
  total += filled.list[0];
  assert(total === 42, "compound assignment with an element it was given");
  assert((filled.list[0] ? "y" : "n") === "y", "an element tests truthy");
  assert(!filled.list[0] === false && (filled.list[0] && 5) === 5, "`!` and `&&` test it");
  const element = filled.list[0];
  let printed = "";
  if (element !== null) printed = `${element}`;
  assert(printed === "41", "a guarded read yields the element");
  let bumped = filled.list[0];
  bumped++;
  assert(bumped === 42, "`++` on a binding holding an element");
  let replaced = 3;
  replaced = filled.list[0];
  assert(replaced === 41, "a variable assigned an element reads it");
  switch (element) {
    default:
      printed = "default";
  }
  assert(printed === "default", "a switch over an element runs its default");
}
