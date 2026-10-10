// A cast is accepted when some member of the source and some member of the
// target are related, as tsc's comparability is: the whole types needn't be.
type Letter = "a" | "b";
type Letters = Letter[] | Letter;

function toLetters(text: string): Letters {
  return text as Letters;
}

function toPrimitive(value: number | boolean): string | number {
  return value as string | number;
}

function toNullableNumber(text: string | null): number | null {
  return text as number | null;
}

function main(): void {
  assert(toLetters("a") === "a", "string as Letter[] | Letter keeps the value");
  assert(toPrimitive(4) === 4, "number | boolean as string | number keeps a number");
  assert(toNullableNumber(null) === null, "a union whose only overlap is null keeps null");
}
