function main(): void { const f = (x: number = 4): number => x; assert(f() === 4, "arrow default"); assert(f(8) === 8, "explicit arrow argument"); }
