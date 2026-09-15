function main(): void {
  const json = JSON.stringify({ b: [1, 2], a: "x" }, null, 2);
  assert(json === "{\n  \"a\": \"x\",\n  \"b\": [\n    1,\n    2\n  ]\n}", "number space pretty prints");
}
