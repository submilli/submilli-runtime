function main(): void {
  const __dst_0 = "USER";
  const [v] = [1];
  assert(__dst_0 === "USER", "pattern temporary cannot capture user binding");
  assert(v === 1);
}
