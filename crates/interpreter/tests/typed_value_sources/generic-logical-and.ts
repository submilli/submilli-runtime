let calls = 0;

function countCalls(): number {
  return calls;
}

function text(): string {
  calls++;
  return "taken";
}

function flag(): boolean {
  calls++;
  return true;
}

function booleanAnd<T>(value: T): string {
  if (typeof value === "boolean") return String(value && text());
  return "other";
}

function numberAnd<T>(value: T): string {
  if (typeof value === "number") return String(value && flag());
  return "other";
}

function main(): number {
  if (booleanAnd(false) !== "false") return 1;
  if (numberAnd(0) !== "0") return 2;
  if (numberAnd(0 / 0) !== "NaN") return 3;
  if (countCalls() !== 0) return 4;
  if (booleanAnd(true) !== "taken") return 5;
  if (numberAnd(2) !== "true") return 6;
  if (booleanAnd(2) !== "other") return 7;
  return countCalls() === 2 ? 0 : 8;
}
