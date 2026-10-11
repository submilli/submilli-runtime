// Optional elements are supported; tuple rest elements still report a focused error.
// expect-error-count: 2
// expect-error: rest elements in tuple types are not supported
type LabeledRest = [a: number, ...rest: string[]];
type Rest = [number, ...string[]];

function main(): void {}
