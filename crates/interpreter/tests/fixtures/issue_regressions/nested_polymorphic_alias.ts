type Chain<T> = { v: T; next: Chain<Chain<T>> | null };
function main(): void { const tip: Chain<number> = { v: 1, next: null }; const nested: Chain<Chain<number>> = { v: tip, next: null }; assert(nested.v.v === 1, "nested alias"); }
