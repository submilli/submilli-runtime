// expect-error: expected `void`, got `number`
function main(): void { const f = (): void => 1; f(); }
