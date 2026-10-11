// expect-error: expected `number`, got `undefined`
class InferredNumberValue {
  constructor(public value = 1) {}
}
function main(): void {
  const value = new InferredNumberValue();
  value.value = undefined;
}
