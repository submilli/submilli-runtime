// expect-error: cannot infer the type of `xs` at this read
function main(): number[] {
  const out = [];
  for (const n of [1, 2]) {
    const xs = [];
    for (const m of [10, 20]) {
      const read = xs;
      out.push(read.length);
    }
    xs.push(n);
  }
  return out;
}
