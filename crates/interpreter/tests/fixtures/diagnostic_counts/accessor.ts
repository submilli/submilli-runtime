// expect-error-count: 3
// expect-error: duplicate member `m`
// expect-error: `void` cannot be an array element
// expect-error: expected `number`, got `string`
class C { m(): void {} set m(x: void[]) { const bad: number = "s"; } }
export function main(): void {}
