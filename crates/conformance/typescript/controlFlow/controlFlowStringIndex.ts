// @target: es2015
// @strict: true
type A = {
    other: number | null;
    [index: string]: number | null
};
const value: A = null as unknown as (A);
if (value.foo !== null) {
    value.foo.toExponential()
    value.other // should still be number | null
    value.bar // should still be number | null
}


function main(): void {}
