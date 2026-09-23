// Tuples have no optional or rest elements, labeled or not; `tsc` accepts all
// four forms. Each reports what is unsupported rather than a stray-token error.
// expect-error-count: 4
// expect-error: optional tuple elements are not supported
// expect-error: rest elements in tuple types are not supported
type LabeledOptional = [a: number, b?: string];
type Optional = [number, string?];
type LabeledRest = [a: number, ...rest: string[]];
type Rest = [number, ...string[]];

function main(): void {}
