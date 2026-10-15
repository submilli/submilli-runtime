// SUB-1823: substr, trimLeft and trimRight, as in tsc's lib and JavaScript.
function main(): void {
  const s = "abcdef";
  assert(s.substr(2) === "cdef");
  assert(s.substr(1, 3) === "bcd");
  assert(s.substr(-2) === "ef" && s.substr(-10, 2) === "ab");
  assert(s.substr(4, 10) === "ef" && s.substr(2, -1) === "" && s.substr(9) === "");
  assert(s.substr(1.7, 2.9) === "bc" && s.substr(NaN, 1) === "a");
  assert(s.substr(0, NaN) === "" && s.substr(0, Infinity) === "abcdef");
  assert(" \t x \n".trimLeft() === "x \n");
  assert(" \t x \n".trimRight() === " \t x");
}
