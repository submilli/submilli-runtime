interface P { a: number; b?: string; }
type Alias = P;
function named(p: P): string { if ("b" in p) { return p.b; } return "missing"; }
function alias(p: Alias): string { if ("b" in p) { return p.b; } return "missing"; }
function inline(p: { a: number; b?: string }): string { if ("b" in p) { return p.b; } return "missing"; }
function nullable(p: { b?: string | null }): string | null { if ("b" in p) { return p.b; } return "missing"; }
function required(p: { b: string | null }): boolean { return "b" in p; }
interface Other { other: boolean; }
function union(p: P | Other): string { if ("b" in p) { return p.b; } return "missing"; }
function main(): void {
    assert(named({a:1,b:"ok"}) === "ok", "named present");
    assert(named({a:1}) === "missing", "named omitted");
    assert(alias({a:1,b:"ok"}) === "ok", "alias present");
    assert(alias({a:1}) === "missing", "alias omitted");
    assert(inline({a:1,b:"ok"}) === "ok", "inline present");
    assert(inline({a:1}) === "missing", "inline omitted");
    assert(nullable({b:null}) === null, "optional null is present");
    assert(required({b:null}), "required null is present");
    assert(union({a:1,b:"ok"}) === "ok", "union present");
    assert(union({a:1}) === "missing", "optional union member can be absent");
    assert(union({other:true}) === "missing", "union lacks member");
}
