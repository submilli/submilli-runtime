// expect-error: expected `Chain<number>`, got `Chain<string>`
type Chain<T> = { v: T; next: Chain<Chain<T>> | null };
function main(): void { const tip: Chain<string> = {v:"wrong",next:null}; const nested: Chain<Chain<number>> = {v:tip,next:null}; }
