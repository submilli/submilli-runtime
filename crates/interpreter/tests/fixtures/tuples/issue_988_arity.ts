// expect-error: expected `[string, number] | [string, boolean]`
function main(): [string, number] | [string, boolean] { return ["ok"]; }
