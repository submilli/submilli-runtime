// Expression-position `x.f++` on a class receiver (statement position desugars
// to an assignment instead, so it takes a different path). The slot is resolved
// statically, like every other class field access: a subclass that redeclares a
// parent field name has two entries of that name in the field-names array, and
// a by-name scan would find the parent's.
class Base {
  n: number = 1;
  tally: bigint = 3n;
}

class Derived extends Base {
  n: number = 100;
  own: number = 7;
}

function main(): void {
  const d = new Derived();
  const shadowed = d.n++;
  assert(shadowed === 100);
  assert(d.n === 101);
  d.n += 5;
  assert(d.n === 106);

  const own = d.own++;
  assert(own === 7);
  assert(d.own === 8);

  const inherited = d.tally++;
  assert(inherited === 3n);
  assert(d.tally === 4n);

  const sum = d.own-- + d.own--;
  assert(sum === 15);
  assert(d.own === 6);

  const arr: Derived[] = [d];
  assert(arr[0].own++ === 6);
  assert(d.own === 7);

  // Inside a loop and inside try/finally — the read-modify-write straddles no
  // block boundary, but the anonymous locals it declares live in an enclosing one.
  let seen: number = 0;
  while (d.own < 10) {
    seen = seen + d.own++;
  }
  assert(seen === 24);
  assert(d.own === 10);

  try {
    assert(d.own++ === 10);
  } finally {
    d.own++;
  }
  assert(d.own === 12);
}
