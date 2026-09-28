// A brace-less body may be any statement but a declaration, including one
// using a variable named `type`.
function main(): void {
  let type: number[] = [1, 2];
  if (type.length > 0) type[0] = 5;
  let i = 0;
  while (i < 1) type[i++] += 1;
  if (i > 0) type = [type[0], 7];
  for (const x of [1]) (() => { i += x; })();
  console.log(type.join(","), i);
}
