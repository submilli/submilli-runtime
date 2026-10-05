// A write the checker rejects doesn't narrow its target: it keeps its declared
// type, as in TypeScript, so the mistakes after it are still reported. (`++`
// on an array reports twice, where tsc reports once.)
// expect-error: expected `number`, got `string`
// expect-error: expected `string`, got `number`
// expect-error: expected `number | string`, got `boolean`
// expect-error: expected `number`, got `number | string`
// expect-error: postfix `++` expects `number` or `bigint`, found `number[]`
// expect-error: expected `number[]`, got `number`
// expect-error: expected `string | null`, got `number`
// expect-error: expected `string`, got `string | null`
// expect-error: expected `string`, got `number`
// expect-error-count: 10
class Box {
  value: number | string = 1;
}

function writes(a: number, b: string, box: Box, items: number[]): void {
  a = b;
  b = a;
  box.value = true;
  const fromField: number = box.value;
  items++;
  const count: number = items.length;
  let rejected: string | null = 5;
  const fromInitializer: string = rejected;
}

let moduleCount: number = 0;
moduleCount = "none";
const moduleText: string = moduleCount;

function main(): void {
  writes(1, "x", new Box(), [1]);
}
