class Key {
  constructor(private text: string) {}
  toString(): string { return this.text; }
}
let index: number | Key = 0;
function change(text: string): boolean { index = new Key(text); return false; }
function check(text: string): void {
  const values = [4, 10];
  index = 0;
  let threw = false;
  if (typeof index === "number" && !change(text)) {
    try { values[index] = 7; } catch (error) { threw = error instanceof RangeError; }
  }
  assert(threw);
  assert(values[0] === 4 && values[1] === 10);
}
function main(): void {
  check("01");
  check("-0");
  check("1e0");
  check(" 1 ");
  const values = [4];
  values[-0] = 7;
  assert(values[0] === 7);
}
