// `o[k]` on a receiver that may be `undefined` needs a check first (TS18048).
// expect-error: cannot index into non-array type `T | undefined`
function read<T, K extends keyof T>(value: T | undefined, key: K): T[K] { return value[key]; }
function main(): void {}
