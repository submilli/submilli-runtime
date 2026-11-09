// An empty array grown after a `break` that leaves an unbounded loop:
// the read after the loop sees every element earlier passes added.
function showFeed(page: number): number[] {
  return page < 3 ? [page * 10, page * 10 + 1] : [];
}

function forBreakFirst(): number[] {
  const xs = [];
  for (let p = 0; ; p++) {
    if (p > 1) break;
    xs.push(p);
  }
  return xs;
}

function forInnerForOf(): number[] {
  const xs = [];
  const page = [7, 8];
  for (let p = 0; ; p++) {
    if (p > 1) break;
    for (const t of page) xs.push(t);
  }
  return xs;
}

function whileTrueInnerForOf(): number[] {
  const xs = [];
  let p = 0;
  while (true) {
    if (p > 1) break;
    for (const t of [p]) xs.push(t);
    p++;
  }
  return xs;
}

function boundedInnerForOf(): number[] {
  const xs = [];
  for (let p = 0; p < 2; p++) {
    for (const t of [p]) xs.push(t);
  }
  return xs;
}

function breakAfterGrowth(): number[] {
  const xs = [];
  for (let p = 0; ; p++) {
    for (const t of [p]) xs.push(t);
    if (p > 1) break;
  }
  return xs;
}

function pagination(): number[] {
  const feed = [];
  for (let page = 0; ; page++) {
    const items = showFeed(page);
    if (items.length === 0) break;
    for (const t of items) feed.push(t);
  }
  return feed;
}

function breaksSeeDifferentElementTypes(): (number | string)[] {
  const xs = [];
  let p = 0;
  for (;;) {
    p++;
    if (p > 4) break;
    xs.push(p);
    if (p > 2) break;
    xs.push("s" + p);
  }
  return xs;
}

function breakInSwitch(): number[] {
  const xs = [];
  let p = 0;
  while (true) {
    p++;
    switch (p) {
      case 1:
        continue;
      case 4:
        break;
      default:
        xs.push(p);
    }
    if (p > 5) break;
  }
  return xs;
}

function breakInDoWhile(): number[] {
  const xs = [];
  let p = 0;
  do {
    if (p > 1) break;
    xs.push(p);
    p++;
  } while (true);
  return xs;
}

function breakInNestedLoops(): number[] {
  const out = [];
  for (let o = 0; o < 3; o++) {
    const xs = [];
    for (let p = 0; ; p++) {
      if (p > o) break;
      xs.push(p * 2);
    }
    out.push(xs.length);
  }
  return out;
}

function breakInTry(): number[] {
  const xs = [];
  let p = 0;
  for (;;) {
    try {
      if (p > 1) break;
      xs.push(p);
    } finally {
      p++;
    }
  }
  return xs;
}

function arrayOutsideTwoLoops(): number[] {
  const xs = [];
  for (let o = 0; o < 2; o++) {
    for (let p = 0; ; p++) {
      if (p > o) break;
      xs.push(o * 10 + p);
    }
  }
  return xs;
}

function boundedWhileBreakFirst(): number[] {
  const xs = [];
  let p = 0;
  while (p < 5) {
    if (p > 1) break;
    xs.push(p);
    p++;
  }
  return xs;
}

function continueBeforePush(): number[] {
  const xs = [];
  let p = 0;
  for (;;) {
    p++;
    if (p > 5) break;
    if (p % 2 === 0) continue;
    xs.push(p);
  }
  return xs;
}

function forInBreakFirst(): string[] {
  const keys = [];
  const record: Record<string, number> = { a: 1, b: 2, c: 3 };
  for (const k in record) {
    if (k === "c") break;
    keys.push(k);
  }
  return keys;
}

function main(): void {
  assert(forBreakFirst().join(",") === "0,1", "break before the push");
  assert(forInnerForOf().join(",") === "7,8,7,8", "break before an inner for-of push");
  assert(whileTrueInnerForOf().join(",") === "0,1", "while (true) with a break first");
  assert(boundedInnerForOf().join(",") === "0,1", "bounded loop");
  assert(breakAfterGrowth().join(",") === "0,1,2", "break after the push");
  assert(pagination().join(",") === "0,1,10,11,20,21", "pagination loop");
  assert(
    breaksSeeDifferentElementTypes().join(",") === "1,s1,2,s2,3",
    "two breaks, two element types",
  );
  assert(breakInSwitch().join(",") === "2,3,5,6", "break in a switch and a continue");
  assert(breakInDoWhile().join(",") === "0,1", "do-while with a break first");
  assert(breakInNestedLoops().join(",") === "1,2,3", "break in nested loops");
  assert(breakInTry().join(",") === "0,1", "break inside try");
  assert(arrayOutsideTwoLoops().join(",") === "0,10,11", "array outside two loops");
  assert(boundedWhileBreakFirst().join(",") === "0,1", "bounded while, break first");
  assert(continueBeforePush().join(",") === "1,3,5", "continue before the push");
  assert(forInBreakFirst().join(",") === "a,b", "for-in with a break before the push");
}
