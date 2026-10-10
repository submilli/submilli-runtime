// SUB-1430: unknown annotations use the same catch-all as an untyped binding.
class DetailedError extends Error {
    constructor(public detail: string) { super("detailed"); }
}

type CatchValue = unknown;

function main(): void {
    let message = "";
    try {
        throw new Error("plain");
    } catch (e: unknown) {
        message = e.message;
    }
    assert(message === "plain", "unknown catch binds Error");

    let detail = "";
    let finished = false;
    try {
        throw new DetailedError("preserved");
    } catch (e: unknown) {
        if (e instanceof DetailedError) detail = e.detail;
    } finally {
        finished = true;
    }
    assert(detail === "preserved", "subclass fields survive catch-all");
    assert(finished, "finally runs after unknown catch");

    try {
        try {
            throw new TypeError("rethrow");
        } catch (e: unknown) {
            throw e;
        }
    } catch (e: CatchValue) {
        assert(e instanceof TypeError, "alias catch preserves rethrown identity");
        assert(e.message === "rethrow", "alias catch binds Error");
    }

    let selected = "";
    try {
        throw new Error("fallback");
    } catch (e: TypeError) {
        selected = "wrong";
    } catch (e: unknown) {
        selected = e.message;
    }
    assert(selected === "fallback", "unknown catches after a subclass filter");
}
