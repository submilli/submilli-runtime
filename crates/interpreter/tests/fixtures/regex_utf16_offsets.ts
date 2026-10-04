function main(): void {
  const regex = /x/g;
  const first = regex.exec("éx😀x");
  assert(first !== null && first.index === 1, "match index counts UTF-16 units");
  assert(regex.lastIndex === 2, "lastIndex counts UTF-16 units");
  const second = regex.exec("éx😀x");
  assert(second !== null && second.index === 4 && regex.lastIndex === 5, "astral prefix counts two units");
  assert(regex.exec("éx😀x") === null && regex.lastIndex === 0, "failed match resets lastIndex");
  assert("é😀x".search(/x/) === 3, "search returns UTF-16 index");
  const carry = /x/g;
  assert(carry.test("x"), "establish lastIndex");
  const moved = carry.exec("éx");
  assert(moved !== null && moved.index === 1, "lastIndex carries safely to another input");
  const sticky = /x/y;
  assert(sticky.test("x"), "establish sticky lastIndex");
  assert(!sticky.test("😀x") && sticky.lastIndex === 0, "sticky must not round past the requested unit");
  const bmp = "é".matchAll(/(?:)/g);
  assert(bmp.length === 2 && bmp[0].index === 0 && bmp[1].index === 1, "empty matches advance past BMP character");
  const astral = "😀".matchAll(/(?:)/gu);
  assert(astral.length === 2 && astral[0].index === 0 && astral[1].index === 2, "unicode empty matches advance past surrogate pair");
  const tail = "é".matchAll(/$/g);
  assert(tail.length === 1 && tail[0].index === 1, "empty match found later is returned once");
}
