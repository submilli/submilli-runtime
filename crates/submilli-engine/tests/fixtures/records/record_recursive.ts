interface Container { values: Values; }
interface Values { [key: string]: number; }
type Tree = Record<string, Tree | number>;
function main(): void {
  let caught = false;
  try { const value = JSON.parse('{"values":{"x":"bad"}}') as Container; } catch (e: Error) { caught = true; }
  assert(caught);
  const tree = JSON.parse('{"branch":{"leaf":3}}') as Tree;
  const child = tree.branch as Tree;
  assert(child.leaf === 3);
  const direct: Record<string, number> = { x: 1 };
  assert(JSON.stringify(direct) === '{"x":1}');
}
