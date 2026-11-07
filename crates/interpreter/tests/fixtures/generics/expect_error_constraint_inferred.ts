// expect-error: does not satisfy the constraint
function getId<T extends { id: number }>(value: T): number { return value.id; }
function main(): void { getId({ id: "wrong" }); }
