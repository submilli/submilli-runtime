// An object type in a diagnostic shows each field as TypeScript does: `readonly`
// kept, and a name that isn't an identifier quoted.
// expect-error-count: 1
// expect-error: cannot assign to readonly property `x` on `{ "a b": string; default: number; readonly x: number; y?: boolean }`
function move(o: { readonly x: number; "a b": string; y?: boolean; default: number }): void {
  o.x = 1;
}

function main(): void {}
