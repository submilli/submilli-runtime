// expect-error-count: 3
// expect-error: duplicate member `m`
// expect-error: Bogus
// expect-error: expected `number`, got `string`
class C { m(): void {} get m(): Bogus { const bad: number = "s"; return 1; } }
export function main(): void {}
