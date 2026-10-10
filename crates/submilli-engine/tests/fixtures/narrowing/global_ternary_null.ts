let numberValue: number | null = 3;
let stringValue: string | null = "value";
function main(): void {
  if (numberValue !== null) {
    assert((numberValue === null ? 1 : 2) === 2);
    assert((numberValue !== null ? numberValue : 0) === 3);
    assert(numberValue !== null && numberValue > 1);
  }
  if (stringValue !== null) {
    assert((stringValue === null ? "missing" : stringValue) === "value");
    assert(stringValue === null || stringValue.length === 5);
  }
}
