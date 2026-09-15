function main(): void {
    const s = "a
b";
    assert(s === "a\nb", "raw newline is the string's newline");
    const t = 'line one
line two
line three';
    assert(t.split("\n").length === 3, "raw newlines in single quotes too");
}
