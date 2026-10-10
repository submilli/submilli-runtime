// expect-error: an index signature must declare exactly one parameter
// expect-error-count: 3
type Scores = {
  total: number
  [];
};

interface Totals {
  count: number
  []: number;
}

let weights: { readonly []: number } = {};

function main(): void {}
