// expect-error: expected `{ tag: string }`, got `Hidden`
class Hidden { private tag: string = "a"; }
function main(): void { const x: {tag: string} = new Hidden(); }
