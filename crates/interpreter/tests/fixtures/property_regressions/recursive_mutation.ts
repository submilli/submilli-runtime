interface Link { tag: number; next: Link | null; }
function main(): void {
    const first: Link = { tag: 1, next: null };
    const second: Link = { tag: 2, next: null };
    const empty: Link = { tag: 1, next: null };
    first.next = second;
    assert(JSON.stringify(first) === '{"next":{"next":null,"tag":2},"tag":1}', "serialization sees mutation");
    assert(first !== empty, "equality sees mutation");
    const expected: Link = { tag: 1, next: second };
    assert(first === expected, "equal updated graphs");
    const keys: Set<Link> = new Set<Link>();
    keys.add(first);
    assert(keys.has(expected), "hash agrees with equality");
    expected.next = null;
    assert(JSON.stringify(expected) === '{"next":null,"tag":1}', "clearing recursive field");
}
