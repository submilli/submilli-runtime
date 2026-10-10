// expect-error: expected `:` after field name
// `readonly` followed by a line break is a member named `readonly`, not a
// modifier, as in TypeScript, so this index signature has a stray name above it.
type F = {
  readonly
  [k: string]: number
};

function main(): void {}
