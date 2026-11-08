// expect-error: `Readonly` cannot be applied to `Node` inside its own definition
interface Node { id: number; next: Readonly<Node> | null }
function main(): void {}
