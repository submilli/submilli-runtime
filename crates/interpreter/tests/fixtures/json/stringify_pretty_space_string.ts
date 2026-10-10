function main(): void {
  const json = JSON.stringify({ a: { b: true } }, null, "\t");
  assert(json === "{\n\t\"a\": {\n\t\t\"b\": true\n\t}\n}", "string space pretty prints");
}
