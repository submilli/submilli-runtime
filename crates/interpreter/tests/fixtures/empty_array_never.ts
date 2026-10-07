// An unannotated `[]` holds no element, so it is `never[]`, as in tsc: it can be
// read, iterated, spread and nested, and fits any array type it meets later.
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
}
