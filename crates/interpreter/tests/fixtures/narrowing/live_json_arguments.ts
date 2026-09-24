interface Text { readonly value: string | null; }
class Changed implements Text {
  private reads: number = 0;
  get value(): string | null {
    this.reads += 1;
    return this.reads === 1 ? "0" : null;
  }
}
function parse(value: Text): unknown {
  if (value.value !== null) { return JSON.parse(value.value); }
  return false;
}
function stringSpace(values: string[]): string {
  return JSON.stringify({ a: 1 }, null, values[0]);
}
function numberSpace(values: number[]): string {
  return JSON.stringify({ a: 1 }, null, values[0]);
}
function changedSpace(value: Text): string {
  if (value.value !== null) { return JSON.stringify({ a: 1 }, null, value.value); }
  return "wrong";
}
function main(): void {
  assert(parse(new Changed()) === null);
  assert(stringSpace([" "]) === "{\n \"a\": 1\n}");
  assert(numberSpace([2]) === "{\n  \"a\": 1\n}");
  assert(changedSpace(new Changed()) === '{"a":1}');
}
