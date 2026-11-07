// expect-error: expected `number`, got `string`
// expect-error-count: 1
function main(): void {
  const pick: (x: number) => number = x => {
    if (x > 0) return x;
    return "neg";
  };
  pick(1);
}
