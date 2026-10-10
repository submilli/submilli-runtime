function assign(target: { left?: number | null; right?: number | null }, key: "left" | "right"): void {
    assert(!(key in target));
    target[key] = null;
    assert(key in target);
    assert(Object.hasOwn(target, key));
    assert(Object.keys(target).length === 1);
    assert(JSON.stringify(target) === (key === "left" ? '{"left":null}' : '{"right":null}'));
}
function main(): void {
    assign({}, "left");
    assign({}, "right");
}
