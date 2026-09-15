// A tuple is an `$Array` at runtime, so `for-of` reads it with the array
// desugar; the loop variable is the union of the positions' types.
function main(): void {
  const pair: [number, number] = [1, 2];
  let sum = 0;
  for (const e of pair) {
    sum = sum + e;
  }
  assert(sum === 3, "numeric tuple sums");

  // A heterogeneous tuple binds the union of its positions, so the loop variable
  // needs narrowing before either member's operations are reachable.
  const mixed: [number, string] = [7, "x"];
  const seen: string[] = [];
  for (const e of mixed) {
    if (typeof e === "number") {
      seen.push(e.toString());
    } else {
      seen.push(e);
    }
  }
  assert(seen.length === 2, "mixed tuple yields both positions");
  assert(seen[0] === "7", "first position");
  assert(seen[1] === "x", "second position");

  const single: [string] = ["only"];
  let count = 0;
  for (const e of single) {
    assert(e === "only", "single-element tuple");
    count = count + 1;
  }
  assert(count === 1, "one iteration");

  const nested: [number[], number[]] = [[1, 2], [3]];
  let total = 0;
  for (const inner of nested) {
    total = total + inner.length;
  }
  assert(total === 3, "tuple of arrays");

  const alias: PairAlias = [1, 2];
  let sa = 0;
  for (const e of alias) {
    sa = sa + e;
  }
  assert(sa === 3, "for-of over a tuple alias");

  const holder: Holder = { pair: [1, 2] };
  let sf = 0;
  for (const e of holder.pair) {
    sf = sf + e;
  }
  assert(sf === 3, "for-of over a tuple field");

  let sr = 0;
  for (const e of makePair()) {
    sr = sr + e;
  }
  assert(sr === 3, "for-of over a returned tuple");

  // A homogeneous tuple's positions collapse to one type, not a redundant union.
  const same: [string, string] = ["a", "b"];
  let joined = "";
  for (const e of same) {
    joined = joined + e.toUpperCase();
  }
  assert(joined === "AB", "homogeneous tuple binds the element type directly");

  const deep: [[number, number], [number, number]] = [[1, 2], [3, 4]];
  let sd = 0;
  for (const inner of deep) {
    for (const e of inner) {
      sd = sd + e;
    }
  }
  assert(sd === 10, "for-of over a tuple of tuples");

  const three: [number, number, number] = [1, 2, 3];
  let sb = 0;
  for (const e of three) {
    if (e === 2) {
      continue;
    }
    if (e === 3) {
      break;
    }
    sb = sb + e;
  }
  assert(sb === 1, "break and continue inside a tuple for-of");
}

type PairAlias = [number, number];

interface Holder {
  pair: [number, number];
}

function makePair(): [number, number] {
  return [1, 2];
}
