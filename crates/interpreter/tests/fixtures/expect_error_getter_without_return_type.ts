// expect-error-count: 1
// expect-error: expected `:` and return type
// A getter's type is written, as a method's is, rather than inferred from its
// body. Uses of it report nothing more (SUB-1155).
class C1 {
  get p() {
    return "x";
  }
}

function main(): void {
  const n: number = new C1().p;
  console.log(new C1().p, n);
}
