const RADIX: number = 16;
function parse_at(x: number, base: number = RADIX): number { return x + base; }
function main(): void { assert(parse_at(1) === 17, "default resolves declaration global"); }
