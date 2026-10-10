function main(): void {
  const __for_first_0 = "USER";
  let out = "";
  for (let i = 0; i < 2; i++) { out += __for_first_0 + i.toString(); }
  assert(out === "USER0USER1", "for temporary cannot capture user binding");
}
