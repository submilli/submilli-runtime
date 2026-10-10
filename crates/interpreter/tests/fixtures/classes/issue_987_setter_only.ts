// expect-error: expected `{ tag: string }`, got `SetterOnly`
class SetterOnly { set tag(value: string) {} }
function main(): void { const x: {tag: string} = new SetterOnly(); }
