interface Named { name?: string; method(): number; }
class Empty implements Named { name?: string; method(): number { return 1; } }
class Getter implements Named {
    calls: number = 0;
    get name(): string { this.calls++; return "name"; }
    set name(value: string) {}
    method(): number { return 1; }
}
function hasName(value: Named): boolean { return "name" in value; }
function hasUnknown(value: unknown): boolean { return value !== null && "name" in value; }
function main(): void {
    assert(!hasName(new Empty()), "method-bearing interface optional null absent");
    const getter = new Getter();
    assert(hasName(getter), "accessor counts as present");
    assert(getter.calls === 0, "presence does not invoke getter");
    assert(hasUnknown(getter), "unknown accessor counts as present");
    assert(hasUnknown({name:null}), "required unknown null present");
    assert(!hasUnknown({}), "unknown absent field");
}
