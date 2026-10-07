// expect-error: expected `:` and a type for the class field
// A class modifier other than `static` must share a line with the member it
// modifies; otherwise it is the member's own name, as in TypeScript.
class C {
  readonly
  a: number = 1;
}

function main(): void {}
