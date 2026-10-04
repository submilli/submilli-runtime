function main(): void {
  let caught = false;
  try { BigInt("7".repeat(65537)); }
  catch (e: RangeError) { caught = e.message.includes("decimal input"); }
  assert(caught, "decimal input has a work bound before conversion");
  const n = BigInt("12345678901234567890");
  assert(n.toString() === "12345678901234567890");
  assert(`${n}` === "12345678901234567890", "implicit formatting uses the same conversion");
}
