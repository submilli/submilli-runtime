function main(): void {
    const left: Record<string, number> = {};
    const right: Record<string, number> = {};
    for (let i = 0; i < 256; i++) {
        left[i.toString()] = i;
        right[(255 - i).toString()] = 255 - i;
    }
    assert(Object.keys(left).length === 256);
    assert(Object.values(left).length === 256);
    assert(Object.entries(left).length === 256);
    for (let i = 0; i < 256; i++) {
        assert(left[i.toString()] === i);
        assert(right[i.toString()] === i);
    }
    assert(Object.is(left, right));
    assert(Object.is(right, left));
    const keys = new Map<unknown, number>();
    keys.set(left, 7);
    assert(keys.get(right) === 7);
    const parsed = JSON.parse(JSON.stringify(left));
    assert(Object.is(left, parsed));
    assert(Object.is(parsed, left));
    assert(keys.get(parsed) === 7);

    const literal = { alpha: 1, beta: 2 };
    const dynamic: Record<string, number> = {};
    dynamic.beta = 2;
    dynamic.alpha = 1;
    assert(Object.is(literal, dynamic));
    assert(Object.is(dynamic, literal));
    keys.set(literal, 8);
    assert(keys.get(dynamic) === 8);

    const missing: { absent?: number | null } = {};
    keys.set(missing, 9);
    const name: "absent" = "absent";
    assert(!(name in missing));
    assert(keys.get(missing) === 9);
    const presentNull: { absent?: number | null } = { absent: null };
    assert(!Object.is(missing, presentNull));
    assert(!Object.is(presentNull, missing));
    keys.set(presentNull, 12);
    assert(keys.get(missing) === 9);
    assert(keys.get(presentNull) === 12);

    const optional: { alpha: number; absent?: number | null } = { alpha: 1 };
    const onlyAlpha: Record<string, number> = {};
    onlyAlpha.alpha = 1;
    assert(Object.is(optional, onlyAlpha));
    assert(Object.is(onlyAlpha, optional));
    const forward = new Map<unknown, number>();
    forward.set(optional, 10);
    assert(forward.get(onlyAlpha) === 10);
    const reverse = new Map<unknown, number>();
    reverse.set(onlyAlpha, 11);
    assert(reverse.get(optional) === 11);
    const forwardSet = new Set<unknown>();
    forwardSet.add(optional);
    assert(forwardSet.has(onlyAlpha));
    const reverseSet = new Set<unknown>();
    reverseSet.add(onlyAlpha);
    assert(reverseSet.has(optional));

    const unicode: Record<string, number> = {};
    const surrogate = String.fromCharCode(0xd800);
    unicode[surrogate] = 1;
    unicode["\ufffd"] = 2;
    unicode["😀"] = 3;
    assert(unicode[surrogate] === 1);
    assert(unicode["\ufffd"] === 2);
    assert(unicode["😀"] === 3);
    assert(Object.keys(unicode).length === 3);
}
