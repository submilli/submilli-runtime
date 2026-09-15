interface PropertyValue {
    name: string;
    values?: string[];
}

interface PropertyBag {
    values?: PropertyValue[];
    custom?: Map<string, unknown>;
}

function propertyCount(properties: PropertyBag | null): number {
    if (properties === null) return 0;
    const actual = properties as PropertyBag;
    let count = 0;
    if (actual.values !== null) {
        for (const property of actual.values) {
            count = count + 1;
        }
    }
    if (actual.custom !== null) {
        count = count + actual.custom.size;
    }
    return count;
}

function main(): number {
    const custom = new Map<string, unknown>();
    const extra: unknown = 1;
    custom.set("extra", extra);
    const bag: PropertyBag = {
        values: [{ name: "title" }, { name: "status" }],
        custom: custom,
    };
    assert(propertyCount(bag) === 3);
    assert(propertyCount(null) === 0);

    const m: Map<string, number> | null = new Map<string, number>();
    if (m === null) return 0;
    const same = m as Map<string, number>;
    same.set("a", 1);
    assert(same.size === 1);
    return propertyCount(bag);
}
