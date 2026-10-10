interface Optional { kind: "optional"; value?: number; }
interface RequiredPresent { kind: "required"; value: number | null; }
function unknownPresence(value: unknown): boolean { return "value" in value; }
function mixedPresence(value: Optional | RequiredPresent): boolean { return "value" in value; }
function describe(value: Optional | RequiredPresent): string {
    if ("value" in value) { return value.kind; }
    return value.kind;
}
function main(): void {
    const absent: Optional = {kind:"optional"};
    const present: RequiredPresent = {kind:"required",value:null};
    assert(!unknownPresence(absent), "unknown preserves optional absence");
    assert(unknownPresence(present), "unknown preserves required null");
    assert(!mixedPresence(absent), "mixed union optional absent");
    assert(mixedPresence(present), "mixed union required null present");
    assert(describe(absent) === "optional", "false branch retains optional member");
    assert(describe(present) === "required", "required nullable stays in true branch");
}
