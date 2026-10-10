// Indexing through a type alias. An alias of an array, tuple, or `Uint8Array`
// is indexable — and assignable — on the same terms as the type it names, on
// both the typecheck and the codegen side: codegen forks on the receiver's
// type, so a fork that compared the alias unpeeled would emit array
// instructions against a `Uint8Array` value.
type Bytes = Uint8Array;
type Nums = number[];
type Pair = [string, number];
type Chained = Pair;

function firstByte(b: Bytes): number {
  return b[0];
}

function main(): void {
  const b: Bytes = new Uint8Array([2, 5, 12]);
  assert(b[0] === 2, "read through a Uint8Array alias");
  assert(firstByte(b) === 2, "aliased Uint8Array as a parameter type");
  b[1] = 250;
  assert(b[1] === 250, "write through a Uint8Array alias");
  b[1] += 10;
  assert(b[1] === 4, "compound assign truncates to 8 bits");

  const a: Nums = [1, 2, 3];
  assert(a[2] === 3, "read through an array alias");
  a[0] = 9;
  assert(a[0] === 9, "write through an array alias");
  a[0]++;
  assert(a[0] === 10, "postfix through an array alias");

  const p: Pair = ["x", 1];
  assert(p[0] === "x", "read through a tuple alias");
  const [s, n] = p;
  assert(s === "x" && n === 1, "destructure through a tuple alias");

  const c: Chained = ["y", 2];
  assert(c[1] === 2, "read through a chained alias");
}
