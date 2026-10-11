// `[]` is a `never[]` under an expected array type too, as in tsc, and fits
// the slot it lands in: a binding, a field, a branch, a return, an argument
// or a cast. Writing one to a binding narrows it to the declared array type,
// so elements can still be added.
function first<T>(xs: T[]): T[] {
  return xs;
}

function main(): void {
  let names: string[] = [];
  names = [];
  names.push("a");
  assert(names.length === 1, "a reassigned binding");

  let maybe: number[] | null = [];
  maybe.push(1);
  assert(maybe.length === 1, "a union binding narrows to its array member");

  let holder: { tags: string[] } = { tags: [] };
  holder = { tags: [] };
  holder.tags.push("t");
  assert(holder.tags.length === 1, "a field");

  const flag = Math.random() < 2;
  const branch: string[] = flag ? [] : ["z"];
  branch.push("q");
  assert(branch.length === 1, "a conditional branch");

  const make: () => number[] = () => [];
  const made = make();
  made.push(1);
  assert(made.length === 1, "a function literal's return");

  const passed = first<number>([]);
  passed.push(2);
  assert(passed.length === 1, "an argument");

  const cast = [] as string[];
  cast.push("c");
  const nested: string[][] = [[]];
  nested[0].push("n");
  assert(cast.length === 1 && nested[0].length === 1, "a cast and a nested literal");
}
