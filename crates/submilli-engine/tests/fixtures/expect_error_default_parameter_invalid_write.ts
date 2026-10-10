// expect-error: expected `number`, got `undefined`
function f(value: number = 4): void { value = undefined; }
function main(): void { f(); }
