function nothing(): void {}
function get(): unknown { return nothing(); }
function main(): void { assert(get() === undefined, "void return is a runtime value"); }
