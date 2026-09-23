// `readonly` as a type operator applies only to array and tuple types, as in
// `tsc` (TS1354), and only when written as one: a parenthesized operand is
// rejected too. Object properties take the `readonly` property modifier.
// expect-error-count: 2
// expect-error: `readonly` only applies to array and tuple types
type Name = readonly string;
type Grouped = readonly (number[]);
type Nested = readonly (number[])[];

function main(): void {}
