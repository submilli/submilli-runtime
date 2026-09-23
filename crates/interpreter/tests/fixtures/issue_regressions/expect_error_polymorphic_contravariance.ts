// expect-error: expected `Consumer<unknown>`, got `Consumer<number>`
type Consumer<T> = { use: (value: T) => void; next: Consumer<Consumer<T>> | null };
function widen(c: Consumer<number>): Consumer<unknown> { return c; }
function main(): void { const c: Consumer<number> = { use: (n: number): void => { console.log(n); }, next: null }; widen(c); }
