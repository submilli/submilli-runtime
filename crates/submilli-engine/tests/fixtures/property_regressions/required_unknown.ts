type RequiredField = { n: number; u: unknown };
function main(): void {
    const missing: unknown = JSON.parse('{"n":1}');
    let rejected = false;
    try { const value = missing as RequiredField; } catch (error) { rejected = error instanceof TypeError; }
    assert(rejected, "required unknown rejects missing field");
    const present: unknown = JSON.parse('{"n":1,"u":null}');
    const value = present as RequiredField;
    assert(value.u === null, "present null satisfies unknown");
}
