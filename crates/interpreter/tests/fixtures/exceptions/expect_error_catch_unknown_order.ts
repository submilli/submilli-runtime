// expect-error: unreachable `catch` clause
function main(): void {
    try { throw new Error("boom"); }
    catch (e: unknown) { }
    catch (e: TypeError) { }
}
