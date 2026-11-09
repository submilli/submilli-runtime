// expect-error: `Partial` cannot be applied to `Tree` inside its own definition
type Tree = { value: number; children: Partial<Tree>[] };
function main(): void { const tree: Tree = { value: 1, children: [] }; }
