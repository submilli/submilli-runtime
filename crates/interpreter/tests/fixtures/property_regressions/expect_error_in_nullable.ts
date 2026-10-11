// expect-error: expected `string`, got `string | null | undefined`
function read(p: { b?: string | null }): string {
    if ("b" in p) { return p.b; }
    return "";
}
function main(): void {}
