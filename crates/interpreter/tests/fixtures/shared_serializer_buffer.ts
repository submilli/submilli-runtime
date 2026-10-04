class Counter { calls: number = 0; }
class CustomJson {
    constructor(private readonly counter: Counter) {}
    toJson(): string { this.counter.calls++; return '{"custom":true}'; }
}
class InheritedCustom extends CustomJson {}
class Node {
    private readonly hidden: number = 9;
    constructor(readonly child: unknown) {}
}
class BrokenJson {
    toJson(): string { throw new Error("serialization failed"); }
}
function main(): void {
    assert(JSON.stringify([{}, { child: {} }]) === '[{},{"child":{}}]');
    assert("x".toJson() === '"x"');
    let value: unknown = "x";
    for (let i = 0; i < 32; i++) { value = [value]; }
    assert(JSON.stringify(value) === "[".repeat(32) + '"x"' + "]".repeat(32));
    assert((value as unknown[]).toString() === "x");
    let object: unknown = "x";
    for (let i = 0; i < 16; i++) { object = new Node(object); }
    assert(JSON.stringify([object]) === "[" + '{"child":'.repeat(16) + '"x"' + "}".repeat(16) + "]");
    const counter = new Counter();
    const custom: unknown[] = [new CustomJson(counter), [new InheritedCustom(counter)]];
    assert(JSON.stringify(custom) === '[{"custom":true},[{"custom":true}]]');
    assert(counter.calls === 2);
    assert(JSON.stringify([new Error("message")]) === "[{}]");
    const closure = { toJson: (): string => '{"closure":true}' };
    assert(JSON.stringify([closure]) === '[{"closure":true}]');
    let failed = false;
    try { JSON.stringify([new BrokenJson()]); }
    catch (error: Error) { failed = error.message === "serialization failed"; }
    assert(failed);
    assert(JSON.stringify([[1, 2], [3]]) === "[[1,2],[3]]");
}
