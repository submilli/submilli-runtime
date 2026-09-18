// expect-error: cannot read field `next`
interface Link { next: Link | null; }
function check(o: { cur: Link | null }, skip: boolean): void {
  for (; o.cur !== null; o.cur = o.cur.next) {
    if (skip) { o.cur = null; continue; }
  }
}
function main(): void { check({ cur: { next: null } }, true); }
