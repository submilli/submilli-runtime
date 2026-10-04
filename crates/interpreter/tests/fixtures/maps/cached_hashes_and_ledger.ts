function main(): number {
    const map = new Map<string, number>();
    const set = new Set<string>();
    for (let i = 0; i < 256; i++) {
        const key = "item-" + i.toString();
        map.set(key, i);
        set.add(key);
    }
    for (let i = 255; i >= 0; i--) {
        const key = "item-" + i.toString();
        assert(map.get(key) === i);
        assert(map.delete(key));
        assert(set.delete(key));
    }
    assert(map.size === 0 && set.size === 0);
    for (let i = 0; i < 128; i++) {
        map.set(i.toString(), i);
        set.add(i.toString());
    }
    let count = 0;
    for (const key of set) {
        assert(map.get(key) === count);
        count += 1;
    }
    assert(count === 128);
    const zeros = new Map<number, string>();
    zeros.set(-0, "zero");
    assert(zeros.get(0) === "zero");
    const identities = new Map<Map<string, number>, number>();
    identities.set(map, 9);
    map.clear();
    assert(identities.get(map) === 9);
    assert(identities.get(new Map<string, number>()) === null);
    const f = (): number => 1;
    const functions = new Map<() => number, number>();
    functions.set(f, 1);
    assert(functions.get(f) === 1);
    assert(functions.get((): number => 1) === null);
    const expressions = new Set<RegExp>();
    const r = /a/g;
    expressions.add(r);
    r.test("a");
    assert(expressions.has(r));
    assert(!expressions.has(/a/g));
    const errors = new Map<Error, number>();
    const firstError = new Error("one");
    errors.set(firstError, 1);
    assert(errors.get(firstError) === 1);
    assert(errors.get(new Error("one")) === null);
    const changingError = new Error("before");
    errors.set(changingError, 2);
    changingError.message = "after";
    changingError.name = "Renamed";
    assert(errors.get(changingError) === 2);
    assert(errors.delete(changingError));
    const structural = new Map<unknown, number>();
    const numeric: { id: number } = { id: -0 };
    const nullable: { id: number | null } = { id: 0 };
    const unknownField: { id: unknown } = { id: 0 };
    structural.set(numeric, 3);
    assert(structural.get({ id: 0 }) === 3);
    assert(structural.get(nullable) === 3);
    assert(structural.get(unknownField) === 3);
    const date = Temporal.PlainDate.from("2020-01-01");
    const month = Temporal.PlainYearMonth.from("2020-01");
    assert(!Object.is(date, month));
    assert(!Object.is(month, date));
    structural.set(date, 4);
    assert(structural.get(month) === null);
    const zones = new Map<Temporal.ZonedDateTime, number>();
    zones.set(Temporal.ZonedDateTime.from("2020-01-01T00:00-05:00[US/Eastern]"), 1);
    assert(zones.get(Temporal.ZonedDateTime.from("2020-01-01T00:00-05:00[America/New_York]")) === 1);
    return 0;
}
