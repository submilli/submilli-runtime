function select(value: { "5": number } | { other: string }): number {
  if (5 in value) return value["5"];
  return -1;
}

function fraction(value: { "1.5": number } | { other: string }): number {
  if (1.5 in value) return value["1.5"];
  return -1;
}

function zero(value: { "0": number } | { other: string }): number {
  if (-0 in value) return value["0"];
  return -1;
}

function main(): void {
  assert(select({ "5": 42 }) === 42);
  assert(select({ other: "no" }) === -1);
  assert(fraction({ "1.5": 15 }) === 15);
  assert(fraction({ other: "no" }) === -1);
  assert(zero({ "0": 10 }) === 10);
  assert(zero({ other: "no" }) === -1);
}
