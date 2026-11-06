// expect-error: index signature keys
function main(): void { const value: { [key: "a" | "b"]: string } = {}; }
