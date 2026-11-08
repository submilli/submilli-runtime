// A form over a later declaration reports a missing member once it is bound.
// expect-error: Property `zz` does not exist on type `B`
// expect-error-count: 1
interface A { v: B["zz"] }
interface B { x: number }
function main(): void {}
