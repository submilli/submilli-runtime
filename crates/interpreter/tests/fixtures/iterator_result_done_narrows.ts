// SUB-543: `IteratorResult` is `{ done: false; value: T } | { done: true; value:
// undefined }`, so testing `done` narrows `value` both ways, as in tsc.
function main(): void {
  const it = [1, 2].values();
  const first = it.next();
  if (!first.done) {
    const n: number = first.value;
    assert(n === 1);
  }
  it.next();
  const last = it.next();
  if (last.done) {
    const v: undefined = last.value;
    assert(v === undefined);
  } else {
    assert(false);
  }
  const yielded: IteratorResult<number> = { done: false, value: 1 };
  const finished: IteratorResult<number> = { done: true, value: undefined };
  assert(!yielded.done && finished.done);
}
