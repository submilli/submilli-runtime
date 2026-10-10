function main(): void {
    for (const n of [128, 256]) {
        const input = "a".repeat(n) + "b";
        const missing = "a".repeat(n / 2) + "c";
        const found = "a".repeat(n / 2) + "b";
        assert(input.indexOf(missing) === -1);
        assert(input.lastIndexOf(missing) === -1);
        assert(!input.includes(missing));
        assert(input.indexOf(found) === n / 2);
        assert(input.lastIndexOf(found) === n / 2);
        assert(input.replace(missing, "x") === input);
        assert(input.replaceAll(missing, "x") === input);
        assert(input.split(missing).length === 1);
    }
    assert("aaaaa".lastIndexOf("aaa") === 2);
    assert("aaaaa".lastIndexOf("aaa", 1) === 1);
    assert("aaaaa".indexOf("aaa", 3) === -1);
    assert("aaa".replaceAll("aa", "x") === "xa");
    assert("ababab".split("ab").join("|") === "|||");
    assert("ababab".split("ab", 2).join("|") === "|");
    assert("abc".replaceAll("", "-") === "-a-b-c-");
    const surrogate = String.fromCharCode(0xd800);
    const unicode = "😀" + surrogate + "😀";
    assert(unicode.indexOf(surrogate) === 2);
    assert(unicode.lastIndexOf("😀") === 3);
    assert(unicode.replaceAll(surrogate, "x") === "😀x😀");
}
