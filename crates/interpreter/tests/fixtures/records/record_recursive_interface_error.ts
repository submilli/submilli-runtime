// expect-error: recursive interface types
interface Tree<T> { [key: string]: Tree<T> | T; }
interface PlainTree { [key: string]: PlainTree | number; }
function main(): void {
  const value: unknown = { branch: { leaf: "wrong" } };
  const generic = value as Tree<number>;
  const plain = value as PlainTree;
}
