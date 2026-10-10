function main(): void {
    const input = "order-1234 shipped to alice@example.com on 2026-10-02; ".repeat(1800);
    const matches = input.matchAll(/[a-z]+@[a-z]+\.com/g);
    let count = 0;
    for (const hit of matches) {
        assert(hit.match === "alice@example.com");
        count += 1;
    }
    assert(count === 1800);
    assert(matches[1799].input === input);

    const unusual = "a" + String.fromCharCode(0xd800) + "z";
    const first = unusual.match(/a/);
    if (first === null) { throw new Error("missing match"); }
    assert(first.input === unusual);
    assert(first.input.charCodeAt(1) === 0xd800);
    const executed = /z/.exec(unusual);
    if (executed === null) { throw new Error("missing exec match"); }
    assert(executed.input === unusual);
    for (const hit of unusual.matchAll(/[az]/g)) {
        assert(hit.input === unusual);
    }
    assert("".matchAll(/x/g).length === 0);
    assert("".matchAll(/(?:)/g)[0].input === "");
}
