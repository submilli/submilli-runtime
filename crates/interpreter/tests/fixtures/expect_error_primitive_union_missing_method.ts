// expect-error: cannot read field `toFixed`
function render(value: number | boolean): string {
  return value.toFixed(2);
}
function main(): void {}
