function apply(f: (n: number) => number, n: number): number { return f(n); }
function main(): void {
    const guard = function self(x: unknown, n: number): x is number {
        if (n === 0) { return typeof x === "number"; }
        if (self(x, n - 1)) { const value: number = x; return value >= 0; }
        return false;
    };
    assert(guard(3, 2), "recursive predicate");
    const multiplier = 2;
    const factorial = function recur(n: number): number {
        if (n < 2) { return multiplier; }
        return n * recur(n - 1);
    };
    assert(factorial(4) === 48, "recursive capture");
    assert(apply(function(n: number) { return n + 1; }, 2) === 3, "anonymous inferred callback");
    const shadow = function same(same: number): number { return same + 1; };
    assert(shadow(3) === 4, "parameter shadows expression name");
    const nested = function self(n: number): number {
        const call = (): number => { if (n === 0) { return 9; } return self(n - 1); };
        return call();
    };
    assert(nested(3) === 9, "nested closure captures self");
    const identity = function self(): boolean { const alias = self; return alias === self; };
    assert(identity(), "self identity is stable");
    const unused = function label(n: number) { return n + 3; };
    assert(unused(2) === 5, "named return inference");
}
