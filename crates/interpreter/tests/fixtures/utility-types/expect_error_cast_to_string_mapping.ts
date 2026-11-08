// expect-error: string mapping types have no run-time check
function main(): void { const text = JSON.parse("\"abc\"") as Uppercase<string>; }
