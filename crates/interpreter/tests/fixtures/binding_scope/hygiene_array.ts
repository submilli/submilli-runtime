function main(): void {
  const __arr_0 = "USER";
  const __i_1 = "INDEX";
  let out = "";
  for (const v of [1, 2]) { out += __arr_0 + __i_1 + v.toString(); }
  assert(out === "USERINDEX1USERINDEX2");
}
