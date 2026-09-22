function main(): void {
  assert((1e21).toString() === "1e+21");
  assert((1e300).toString() === "1e+300");
  assert((1e20).toString() === "100000000000000000000");
  assert((1e-6).toString() === "0.000001");
  assert((1e-7).toString() === "1e-7");
  assert((-1.234e21).toString() === "-1.234e+21");
  assert((5e-324).toString() === "5e-324");
  assert((-0).toString() === "0");
  assert(JSON.stringify([1e21, 1e-7]) === "[1e+21,1e-7]");
}
