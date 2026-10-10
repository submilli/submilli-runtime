type Chain<T> = { v: T; next: Chain<Chain<T>> | null };
function widen(c: Chain<number>): Chain<unknown> { return c; }
function main(): void { const c = widen({v: 1, next: null}); assert(c.v === 1, "covariance"); }
