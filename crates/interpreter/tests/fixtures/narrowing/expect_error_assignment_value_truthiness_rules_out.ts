// Once an assignment's truthiness has ruled out every value of the reference
// it copied, reading that reference there is reading `never`.
// expect-error: cannot read field `toString` on non-object type `never`
// expect-error-count: 1
function describe(x: number | string | boolean): number | string | boolean {
  let y: number | string | boolean = false;
  let z: number | string | boolean = false;
  return typeof x === "string"
    || (z = x)
    || (typeof x === "number" ? x.toString() : (y = x) && x.toString());
}

function main(): void {
  console.log(describe(1), describe("s"), describe(false));
}
