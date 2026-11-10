// @target: es2015
// test for #17069
function sum<T extends Record<K, number>, K extends string>(n: number, v: T, k: K): void {
    n = n + v[k];
    n += v[k]; // += should work the same way
}
function realSum<T extends Record<K, number>, K extends string>(n: number, vs: T[], k: K): void {
    for (const v of vs) {
        n = n + v[k];
        n += v[k];
    }
}


function main(): void {}
