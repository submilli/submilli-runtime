function zero(x: 0): boolean { return x === -0; }
function negate(x: 1, y: 2): boolean { return y === -x; }
function positive(x: 1, y: 2): boolean { return y === +x; }
function main(): void {
 assert(zero(0), "negative zero");
 assert(!negate(1, 2), "negated variable");
 assert(!positive(1, 2), "positive variable");
}
