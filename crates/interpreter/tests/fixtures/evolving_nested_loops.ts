function collectLengths(): number[] {
  const out = [];
  for (const n of [1, 2]) {
    const xs = [];
    for (const m of [10, 20]) xs.push(m);
    const first: number = xs[0];
    assert(first === 10, "nested array element");
    out.push(xs.length);
  }
  return out;
}

function collectOuterValues(): number[] {
  const out = [];
  for (const n of [1, 2]) {
    const xs = [];
    for (const m of [10, 20]) xs.push(m);
    out.push(n);
  }
  return out;
}

function growOuterFirst(): number[] {
  const out = [];
  for (const n of [1, 2]) {
    out.push(n);
    const xs = [];
    for (const m of [10, 20]) xs.push(m);
    assert(xs.length === 2, "inner growth after outer growth");
  }
  return out;
}

function threeLoops(): number[] {
  const out = [];
  for (const n of [1, 2]) {
    const middle = [];
    for (const m of [10, 20]) {
      const inner = [];
      for (const k of [100, 200]) inner.push(k);
      middle.push(inner.length);
    }
    out.push(middle.length);
  }
  return out;
}

function whileLoops(): number[] {
  const out = [];
  let n = 1;
  while (n <= 2) {
    const xs = [];
    let m = 10;
    while (m <= 20) {
      xs.push(m);
      m += 10;
    }
    out.push(xs.length);
    n++;
  }
  return out;
}

function readsPreviousIteration(): number[] {
  const out = [];
  const xs = [];
  for (const n of [1, 2]) {
    out.push(xs.at(0) ?? 0);
    xs.push(n);
  }
  return out;
}

function changingOuterType(): string[] {
  let value: number | string = 1;
  const out = [];
  for (const n of [1, 2]) {
    const xs = [];
    for (const m of [10, 20]) xs.push(value);
    out.push(String(xs[0]));
    value = "s";
  }
  return out;
}

function main(): void {
  assert(collectLengths().join(",") === "2,2", "nested array lengths");
  assert(collectOuterValues().join(",") === "1,2", "outer array grows after inner loop");
  assert(growOuterFirst().join(",") === "1,2", "outer array grows first");
  assert(threeLoops().join(",") === "2,2", "three nested loops");
  assert(whileLoops().join(",") === "2,2", "nested while loops");
  assert(readsPreviousIteration().join(",") === "0,1", "read before later growth");
  assert(changingOuterType().join(",") === "1,s", "outer type changes");
}
