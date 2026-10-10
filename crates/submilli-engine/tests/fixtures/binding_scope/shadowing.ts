function main(): void {
  const a = "outer";
  { const a = "inner"; assert(a === "inner"); }
  assert(a === "outer");
  for (let i = 0; i < 2; i++) { const i = "body"; assert(i === "body"); }
  for (const v of [1]) { const v = "body"; assert(v === "body"); }
}
