// expect-error: different key and value types
// Multiple differently-valued signatures remain unsupported.
function main(): void {
 const a: Record<`a${string}`, number> = {};
 const b: Record<`b${string}`, string> = {};
 const result = { ...a, ...b };
}
