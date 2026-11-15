enum E { Z, O }
enum S { A = "", B = "b" }
function i(v: E.Z | "a"): string { return v ? "T" : "F"; }
function k(v: S.A | 1): string { return v ? "T" : "F"; }
function numeric(v: E): boolean { return v ? true : false; }
function nullable(v: E | null): boolean { return v ? true : false; }
function text(v: S): boolean { return v ? true : false; }
function coalesced(e: E | null, s: "a" | undefined): boolean {
    return (e ?? s) ? true : false;
}
function main(): string {
    const result = i("a").concat(i(E.Z), "|", k(1), k(S.A));
    assert(result === "TF|TF", "enum members use their values for truthiness");
    assert(!numeric(E.Z) && numeric(E.O), "whole numeric enum");
    assert(!nullable(E.Z) && nullable(E.O) && !nullable(null), "nullable enum");
    assert(!text(S.A) && text(S.B), "whole string enum");
    assert(!coalesced(E.Z, "a") && coalesced(null, "a"), "coalesced enum union");
    return result;
}
