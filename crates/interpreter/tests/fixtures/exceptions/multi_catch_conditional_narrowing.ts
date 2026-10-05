function dispatch(errorToThrow: Error): string {
    let result = "";
    try { throw errorToThrow; }
    catch (error: TypeError) {
        if (error.message === "x") result = "type";
        else result = error.name;
    }
    catch (error: Error) {
        if (error.message === "x") result = "caught";
        else result = error.name;
    }
    return result;
}

function main(): void {
    assert(dispatch(new Error("x")) === "caught", "second arm conditional");
    assert(dispatch(new Error("other")) === "Error", "second arm else");
    assert(dispatch(new TypeError("x")) === "type", "first arm conditional");
    assert(dispatch(new TypeError("other")) === "TypeError", "first arm else");

    const error = { message: "outer" };
    let read: () => string = (): string => "unset";
    let finalized = "";
    try { throw new Error("x"); }
    catch (error: Error) {
        if (error.message === "x") {
            read = (): string => error.message;
        } else {
            read = (): string => error.name;
        }
    } finally {
        finalized = error.message;
    }
    assert(read() === "x", "closure retains catch binding");
    assert(finalized === "outer", "finally sees outer binding");
    assert(error.message === "outer", "catch narrowing does not escape");
}
