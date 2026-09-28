function assign(key: "left" | "right"): void {
  const pair: { left: { x: number }; right: { y: number } } = { left: { x: 1 }, right: { y: 2 } };
  const both = { x: 3, y: 4 };
  pair[key] = both;
  if (key === "left") {
    assert(pair.left.x === 3);
    assert(pair.right.y === 2);
  } else {
    assert(pair.left.x === 1);
    assert(pair.right.y === 4);
  }
}
function main(): void { assign("left"); assign("right"); }
