// expect-error: duplicate `catch` clause for `Error`
function main(): void {
    try { throw new Error("boom"); }
    catch (e: unknown) { }
    catch (e: Error) { }
}
