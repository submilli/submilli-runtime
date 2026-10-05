// Assigning a function to a union of function types narrows the variable to
// the member with the most parameters it fits, as TypeScript calls such a
// union with the longest parameter list. A function written for fewer
// parameters ignores the extra arguments.
type One = (x: number) => number;
type Two = (x: number, y: number) => number;

function pick(useTwo: boolean): number {
  let f: One | Two | null = null;
  f = useTwo ? (x: number, y: number): number => x + y : (x: number): number => x + 1;
  return f(1, 2);
}

function main(): void {
  let f: One | Two | null = null;
  f = (x: number): number => x + 1;
  console.log(f(1, 2), pick(true), pick(false));
}
