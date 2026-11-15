function main(): void {
    const it = [1].values();
    const first = it.next();
    assert(JSON.stringify(first) === '{"value":1,"done":false}');
    assert(JSON.stringify(Object.keys(first)) === '["value","done"]');
    let keys = "";
    for (const key in first) { keys += key + ","; }
    assert(keys === "value,done,");
    const last = it.next();
    assert(JSON.stringify(last) === '{"done":true}');
    assert(JSON.stringify(Object.keys(last)) === '["value","done"]');
    assert(last.value === undefined && last.done === true);
    assert(JSON.stringify(new Map<string, number>([["a", 1]]).keys().next()) === '{"value":"a","done":false}');
    assert(JSON.stringify(new Set<number>([2]).values().next()) === '{"value":2,"done":false}');
    assert(JSON.stringify("abc".matchAll(/b/g).next()).startsWith('{"value":'));
    assert(Array.from([1, 2].values()).join(",") === "1,2");
    assert(new Set<number>([1, 2].values()).size === 2);
    let total = 0;
    for (const n of [1, 2].values()) { total += n; }
    for (const n of new Map<string, number>([["a", 3]]).values()) { total += n; }
    for (const n of new Set<number>([4]).values()) { total += n; }
    for (const match of "a1b2".matchAll(/\d/g)) { total += Number(match[0]); }
    assert(total === 13);
    const extended = Object.assign([1].values().next(), { "10": 10, "2": 2, extra: "x" });
    assert(JSON.stringify(extended) === '{"2":2,"10":10,"value":1,"done":false,"extra":"x"}');
    const nested = { result: [3].values().next() };
    assert(JSON.stringify(nested) === '{"result":{"value":3,"done":false}}');
}
