// expect-error: expected `Other<
interface Growing<X> { value: X; next: Growing<X[]> | null }
interface Other<X> { value: X; next: Other<X[]> | null }
function main(): void {
    const source: Growing<number> = { value: 1, next: null };
    const target: Other<string> = source;
}
