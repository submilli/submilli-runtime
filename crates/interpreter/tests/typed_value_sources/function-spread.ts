function five(): number { return 5; }
let calls = 0;
function getFunction(): () => number { calls++; return five; }
function main(): number {
  const f: () => number = five;
  const o = { ...f, z: 1 };
  const empty = { ...five };
  const evaluated = { ...getFunction(), z: 2 };
  if (Object.keys(o).join(",") !== "z" || o.z !== 1) return 1;
  if (Object.keys(empty).length !== 0) return 2;
  if (Object.keys(evaluated).join(",") !== "z" || evaluated.z !== 2 || calls !== 1) return 3;
  return 0;
}
