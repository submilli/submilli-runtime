// @target: es2015
function foo<T, U>(x: T, y: U): void {
    function bar<V>(z: V): void {
        function baz<W>(a: W): void {
            let c: T = null as unknown as (T);
            let d: U = null as unknown as (U);
            let e: V = null as unknown as (V);
        }
    }
}

function main(): void {}
