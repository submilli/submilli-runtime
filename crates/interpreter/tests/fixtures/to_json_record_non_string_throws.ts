// A `toJson` inserted into a `Record` is not type-checked, so it can return a
// value that is not a string. Reading that result as the JSON text throws a
// catchable error rather than ending the run.
function main(): void {
    const record: Record<string, () => number[]> = {};
    record["toJson"] = () => [1, 2];
    let message = "";
    try {
        JSON.stringify(record);
    } catch (e) {
        message = e.message;
    }
    assert(message !== "", "a non-string toJson result throws");

    // A Uint8Array is not a string either, even though its bytes could be read
    // as text.
    const bytes: Record<string, () => Uint8Array> = {};
    bytes["toJson"] = () => Uint8Array.new([65, 66, 67]);
    let bytesMessage = "";
    try {
        JSON.stringify([bytes]);
    } catch (e) {
        bytesMessage = e.message;
    }
    assert(bytesMessage !== "", "a Uint8Array toJson result throws");
}
