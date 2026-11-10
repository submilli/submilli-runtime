// expect-error: would need a tuple with a rest element
function log(level: number, ...parts: string[]): void {}
function main(): void { const args: Parameters<typeof log> = [1]; }
