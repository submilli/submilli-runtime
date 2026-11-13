// SUB-2280: a built-in iterator has no own enumerable keys. JavaScript keeps
// `next` on the prototype, so `Object.keys`, `values`, `entries`, and `for…in`
// see nothing. `for…of` and `next()` still work.
function listed<T>(it: Iterator<T>): string {
    return JSON.stringify(Object.keys(it));
}

function main(): void {
    const values = [1].values();
    assert(listed(values) === "[]", "array values keys");
    assert(JSON.stringify(Object.values(values)) === "[]", "array values values");
    assert(JSON.stringify(Object.entries(values)) === "[]", "array values entries");
    assert(listed([1].keys()) === "[]", "array keys");
    assert(listed([1].entries()) === "[]", "array entries");
    assert(listed(new Map<string, number>([["a", 1]]).entries()) === "[]", "map entries");
    assert(listed(new Map<string, number>().keys()) === "[]", "map keys");
    assert(listed(new Set<number>([1]).values()) === "[]", "set values");
    assert(listed("a1".matchAll(/\d/g)) === "[]", "matchAll");
    assert(Object.hasOwn(values, "next") === false, "hasOwn");
    assert(("next" in values) === true, "in");
    const copy = { ...values };
    assert(JSON.stringify(copy) === "{}", "spread json");
    assert(JSON.stringify(Object.keys(copy)) === "[]", "spread lists nothing");
    const copied = copy.next();
    assert(copied.done === false && copied.value === 1, "spread keeps next");
    assert(JSON.stringify(Object.keys(Object.assign({}, values))) === "[]", "assign from iterator");
    const assigned = Object.assign(values, { next: 1 });
    assert(assigned.next === 1, "assign writes next");
    assert(JSON.stringify(Object.keys(assigned)) === "[\"next\"]", "assign makes next own");
    const again = { ...assigned };
    assert(again.next === 1, "spread of an assigned next");
    assert(JSON.stringify([2].values()) === "{}", "json");
    const fresh = [1].values();
    let seen = "";
    for (const k in fresh) {
        seen += k;
    }
    assert(seen === "", "for-in");
    const step = fresh.next();
    assert(step.done === false && step.value === 1, "next");
    let walked = "";
    for (const n of [7].values()) {
        walked += String(n);
    }
    assert(walked === "7", "for-of");
    const plain = { next: 1 };
    assert(JSON.stringify(Object.keys(plain)) === "[\"next\"]", "a user field named next");
}
