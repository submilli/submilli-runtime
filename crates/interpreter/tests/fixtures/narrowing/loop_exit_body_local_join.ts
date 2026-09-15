// A `while (true)` carries only its break env out, and that env can name a
// binding declared inside the loop body. Such a path must not be installed at
// the join point — the ident it rebinds to no longer exists there.
interface Inner {
  v: string | null;
}

function next(n: number): Inner | null {
  return n > 0 ? { v: "x" } : null;
}

function main(): void {
  let seen = 0;
  while (true) {
    const p: Inner | null = next(seen);
    seen = seen + 1;
    if (p === null) {
      continue;
    }
    if (p.v === null) {
      continue;
    }
    break;
  }
  assert(seen === 2, "first round continued on null, second broke");

  // the same shape with the guards folded into one `&&`
  let rounds = 0;
  while (true) {
    const q: Inner | null = next(rounds);
    rounds = rounds + 1;
    if (q !== null && q.v !== null) {
      break;
    }
  }
  assert(rounds === 2);

  // root declared in a `for` init, loop with no condition
  let hops = 0;
  for (let o: Inner | null = next(0); ; ) {
    hops = hops + 1;
    if (o !== null && o.v !== null) {
      break;
    }
    o = next(hops);
  }
  assert(hops === 2, "for-init root, break-only exit");

  // same, with a `const` init and a field path
  for (const c: Inner | null = next(1); ; ) {
    if (c !== null && c.v !== null) {
      break;
    }
    assert(false, "unreachable — next(1) is non-null");
  }
}
