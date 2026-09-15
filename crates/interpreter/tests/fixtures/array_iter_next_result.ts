// The IteratorResult terminators ({done: true}) are now built host-side in Rust
// (iter_done), not via the Wasm shim. Drive keys()/entries()/values() through
// explicit .next()/.done — extending the explicit-next coverage in
// array_values_host_iter to every Array iterator, and exercising the empty case
// where the very first .next() must already report done. (Value extraction goes
// through for-of, in array_keys_values_entries.)

function main(): void {
  const kit = ["a", "b", "c"].keys();
  let kCount = 0;
  while (!kit.next().done) {
    kCount = kCount + 1;
  }
  assert(kCount === 3, "keys: three yields then done");

  const eit = ["x", "y"].entries();
  let eCount = 0;
  while (!eit.next().done) {
    eCount = eCount + 1;
  }
  assert(eCount === 2, "entries: two yields then done");

  const empty: number[] = [];
  assert(empty.values().next().done, "empty values: first next is done");
  assert(empty.keys().next().done, "empty keys: first next is done");
  assert(empty.entries().next().done, "empty entries: first next is done");
}
