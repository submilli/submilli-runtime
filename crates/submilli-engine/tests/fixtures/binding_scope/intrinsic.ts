function invoke(): number { const assert = (): number => 42; return assert(); }
function main(): void { assert(invoke() === 42); assert(parseInt("17") === 17); }
