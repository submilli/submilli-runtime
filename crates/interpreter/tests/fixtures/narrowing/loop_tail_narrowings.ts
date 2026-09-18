interface Link { value: number; next: Link | null; }

function viaField(o: { cur: Link | null }): number {
  let sum = 0;
  for (; o.cur !== null; o.cur = o.cur.next) {
    sum = sum + o.cur.value;
    continue;
  }
  return sum;
}

class Walker {
  cur: Link | null = null;
  walk(): number {
    let sum = 0;
    for (; this.cur !== null; this.cur = this.cur.next) {
      sum = sum + this.cur.value;
    }
    return sum;
  }
}

function noCondition(list: Link | null): number {
  let sum = 0;
  for (let cur: Link | null = list; ; cur = cur.next) {
    if (cur === null) { break; }
    sum = sum + cur.value;
  }
  return sum;
}

function guard(x: string | null): number {
  let n = 0;
  do {
    if (x === null) { return -1; }
    n = n + 1;
    continue;
  } while (n < 3 && x.length === 2);
  return n;
}

function assigned(x: string | null): number {
  let n = 0;
  do {
    x = "ab";
    n = n + 1;
  } while (n < 3 && x.length === 2);
  return n;
}

function fieldGuard(o: { x: string | null }): number {
  let n = 0;
  do {
    if (o.x === null) { return -1; }
    n = n + 1;
  } while (n < 3 && o.x.length === 2);
  return n;
}

function main(): void {
  const list: Link = { value: 1, next: { value: 2, next: null } };
  assert(viaField({ cur: list }) === 3);
  const walker = new Walker();
  walker.cur = list;
  assert(walker.walk() === 3);
  assert(noCondition(list) === 3);
  assert(guard("ab") === 3);
  assert(guard(null) === -1);
  assert(assigned(null) === 3);
  assert(fieldGuard({ x: "ab" }) === 3);
}
