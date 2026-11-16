function grow(xs: number[]): number {
  xs.push(99);
  return 7;
}

function shrink(xs: number[]): number {
  xs.pop();
  return 8;
}

function main(): void {
  const xs: number[] = [];
  xs[0] = 1;
  xs[1] = 2;
  const alias = xs;
  for (let i = 2; i < 65; i++) xs[xs.length] = i + 1;
  assert(alias.length === 65 && alias[64] === 65, "append updates aliases across capacity growth");
  xs[0] = 10;
  assert(xs.length === 65 && xs[0] === 10, "existing index overwrites without growing");

  const words: string[] = [];
  words[words.length] = "\uD800";
  assert(words.length === 1 && words[0].charCodeAt(0) === 55296, "append preserves UTF-16 units");
  const objects: { value: number }[] = [];
  const object = { value: 3 };
  objects[0] = object;
  object.value = 4;
  assert(objects[0].value === 4, "append preserves object identity");
  const optional: (number | undefined)[] = [];
  optional[0] = undefined;
  optional[1] = 2;
  assert(optional.length === 2 && optional[0] === undefined && optional[1] === 2, "boxed missing values append");

  const changed = [1];
  changed[changed.length] = grow(changed);
  assert(changed.length === 2 && changed[1] === 7, "RHS growth turns append into overwrite");
  changed[0] = grow(changed);
  assert(changed[0] === 7 && changed[2] === 99, "RHS growth retains the existing-index write");
  changed[2] = shrink(changed);
  assert(changed.length === 3 && changed[2] === 8, "RHS shrink turns overwrite into append");

  let order = "";
  function receiver(): number[] { order += "r"; return changed; }
  function index(): number { order += "i"; return changed.length; }
  function value(): number { order += "v"; return 9; }
  receiver()[index()] = value();
  assert(order === "riv" && changed[3] === 9, "operands run once in order");

  const zero: number[] = [];
  zero[-0] = 5;
  assert(zero.length === 1 && zero[0] === 5, "negative zero appends to an empty array");
}
