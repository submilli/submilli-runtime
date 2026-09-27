// @target: es2015
// @strict: true

const a: string | null = null as unknown as (string | null);
const b: string | null = null as unknown as (string | null);
const c: string | null = null as unknown as (string | null);

// should be a syntax error
a ?? b || c;

// should be a syntax error
a || b ?? c;

// should be a syntax error
a ?? b && c;

// should be a syntax error
a && b ?? c;

// Valid according to spec
a ?? (b || c);

// Valid according to spec
(a ?? b) || c;

// Valid according to spec
(a || b) ?? c;

// Valid according to spec
a || (b ?? c);

// Valid according to spec
a ?? (b && c);

// Valid according to spec
(a ?? b) && c;

// Valid according to spec
(a && b) ?? c;

// Valid according to spec
a && (b ?? c);


function main(): void {}
