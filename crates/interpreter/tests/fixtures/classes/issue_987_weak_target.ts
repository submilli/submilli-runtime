// expect-error: expected `{ tag?: string }`, got `Unrelated`
class Unrelated { value: number = 1; }
function main(): void { const x: {tag?: string} = new Unrelated(); }
