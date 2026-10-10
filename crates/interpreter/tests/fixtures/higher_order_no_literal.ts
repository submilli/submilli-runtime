// A higher-order signature whose closure shape has no literal anywhere in the
// module: the `$closure_<sig>` type has to come from the declaration itself.
type Reducer = (acc: number, n: number) => number;

interface Sink {
  emit: (msg: string) => void;
}

function unusedApply(f: (x: number) => number, x: number): number {
  return f(x);
}

function unusedFold(xs: number[], r: Reducer): number {
  let acc = 0;
  for (let i = 0; i < xs.length; i = i + 1) {
    acc = r(acc, xs[i]);
  }
  return acc;
}

function unusedAnnounce(sink: Sink): void {
  sink.emit("hi");
}

function main(): void {
  assert(true, "module compiles with no closure literal");
}
