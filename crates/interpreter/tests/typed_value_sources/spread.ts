function clone<T>(t: T): T { return { ...t }; }
function cloneObject<T extends object>(t: T): T { return { ...t }; }
function cloneFields<T extends { a: number }>(t: T): T { return { ...t }; }
function spreadKeys<T>(t: T): string { const o = { ...t }; return Object.keys(o as unknown as object).join(","); }
function main(): number {
  const original = { a: 2, b: 3 };
  const copied = clone(original);
  copied.a = 9;
  if (original.a !== 2 || copied.a !== 9 || copied.b !== 3) return 1;
  if (cloneObject(original).b !== 3 || cloneFields(original).a !== 2) return 2;
  if (spreadKeys([5, 6]) !== "0,1") return 3;
  if (spreadKeys("ab") !== "0,1") return 4;
  if (spreadKeys(true) !== "") return 5;
  if (spreadKeys(null) !== "") return 6;
  if (spreadKeys(undefined) !== "") return 7;
  return 0;
}
