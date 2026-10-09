// expect-error: expected a value in this condition, got `void`
function nothing(): void {}
function main(): void { if (nothing()) {} }
