// expect-error-count: 2
// expect-error: duplicate member `m`
// expect-error: expected `number`, got `string`
class C { m(): void {} set m(x: void[]) { const bad: number = "s"; } }
export function main(): void {}
