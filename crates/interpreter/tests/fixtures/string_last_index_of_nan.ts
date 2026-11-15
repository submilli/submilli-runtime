function main(): void {
    assert("abc".lastIndexOf("c", NaN) === 2);
    assert("abc".lastIndexOf("", NaN) === 3);
    assert("abc".lastIndexOf("a", NaN) === 0);
    assert("abcb".lastIndexOf("b", 1.9) === 1);
    assert("abcb".lastIndexOf("b") === 3);
    assert("abc".lastIndexOf("c", Infinity) === 2);
    assert("abc".lastIndexOf("a", -Infinity) === 0);
    assert("abc".lastIndexOf("c", -Infinity) === -1);
    assert("abc".lastIndexOf("", -Infinity) === 0);
    assert("".lastIndexOf("", NaN) === 0);
    assert("".lastIndexOf("a", NaN) === -1);
    assert("\ud800x\ud800".lastIndexOf("\ud800", NaN) === 2);
}
