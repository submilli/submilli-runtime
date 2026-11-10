// expect-error: cannot assign through `Readonly<T>`, which only permits reading
function write<T, K extends keyof T>(value: Readonly<T>, key: K, next: T[K]): void { value[key] = next; }
function main(): void {}
