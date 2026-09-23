// `readonly` as a type operator applies only to array and tuple types, as in
// `tsc` (TS1354). Object properties take the `readonly` property modifier.
// expect-error: `readonly` only applies to array and tuple types
type Name = readonly string;

function main(): void {}
