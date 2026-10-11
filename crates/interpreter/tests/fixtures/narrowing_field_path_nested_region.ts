interface PropertyValue {
    name: string;
}

interface PropertyBag {
    values?: PropertyValue[];
    custom?: Map<string, unknown>;
}

function countGuardedInCondition(properties: PropertyBag | null): number {
    if (properties === null) return 0;
    let count = 0;
    if (properties.values !== undefined) {
        for (const property of properties.values) {
            count = count + 1;
        }
    }
    if (properties.custom !== undefined) {
        count = count + properties.custom.size;
    }
    return count;
}

function blockGuard(p: PropertyBag | null): number {
    if (p !== null) {
        if (p.values !== undefined) return p.values.length;
    }
    return -1;
}

function main(): number {
    assert(countGuardedInCondition(null) === 0);
    assert(blockGuard(null) === -1);

    const custom = new Map<string, unknown>();
    const extra: unknown = "x";
    custom.set("extra", extra);
    const bag: PropertyBag = {
        values: [{ name: "title" }, { name: "status" }],
        custom: custom,
    };
    assert(countGuardedInCondition(bag) === 3);
    assert(blockGuard(bag) === 2);

    const empty: PropertyBag = {};
    assert(countGuardedInCondition(empty) === 0);
    assert(blockGuard(empty) === -1);
    return 0;
}
