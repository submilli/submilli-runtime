// expect-error: expected `string`, got `null`
function main(): void { const x: NonNullable<string | null> = null; }
