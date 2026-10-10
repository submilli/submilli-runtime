// Regression: field assignment (`=`, compound `+=`, postfix `++`) on a value
// typed as a named `interface` — not just an inline object type. The typechecker
// resolves the interface's structural shape for the assignment target, and
// codegen routes the write through the shared `$ObjectShape` setter dispatch.

interface Counter {
    n: number;
    note?: string;
}

function main(): void {
    const c: Counter = { n: 1 };

    c.n = 5;
    assert(c.n === 5, "plain assignment to interface field");

    c.n += 3;
    assert(c.n === 8, "compound assignment to interface field");

    c.n++;
    assert(c.n === 9, "postfix increment of interface field");

    // Assigning a previously-absent optional field.
    assert(c.note === undefined, "optional field starts undefined");
    c.note = "done";
    assert(c.note === "done", "assigned optional field reads back");
}
