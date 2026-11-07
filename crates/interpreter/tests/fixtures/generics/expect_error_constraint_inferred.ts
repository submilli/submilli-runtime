// expect-error: expected `{ id: number }`, got `{ id: string }`
function getId<T extends { id: number }>(value: T): number { return value.id; }
function main(): void { getId({ id: "wrong" }); }
