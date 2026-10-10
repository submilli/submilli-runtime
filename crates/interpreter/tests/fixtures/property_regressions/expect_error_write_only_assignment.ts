// expect-error: expected `Bag`, got `WriteOnly`
interface Bag { note: string; }
class WriteOnly { set note(value: string) {} }
function main(): void { const b: Bag = new WriteOnly(); }
