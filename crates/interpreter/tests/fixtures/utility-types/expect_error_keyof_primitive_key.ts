// expect-error: got `"nope"`
function read<T, K extends keyof T>(value: T, key: K): T[K] { return value[key]; }
function main(): void { const v = read(5, "nope"); }
