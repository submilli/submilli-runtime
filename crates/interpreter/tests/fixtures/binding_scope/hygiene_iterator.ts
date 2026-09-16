function main(): void {
  const __it_0 = "USER";
  const __r_1 = "RESULT";
  const __r_narrow_2 = "NARROW";
  let out = "";
  const values = new Set<number>(); values.add(1); values.add(2);
  for (const v of values) { out += __it_0 + __r_1 + __r_narrow_2 + v.toString(); }
  assert(out === "USERRESULTNARROW1USERRESULTNARROW2");
}
