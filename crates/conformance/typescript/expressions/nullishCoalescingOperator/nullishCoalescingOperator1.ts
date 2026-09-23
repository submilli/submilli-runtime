// @target: es2015
// @strict: true
// @allowUnreachableCode: false

const a1: string | null | null = null as unknown as (string | null | null);
const a2: string | null | null = null as unknown as (string | null | null);
const a3: string | null | null = null as unknown as (string | null | null);
const a4: string | null | null = null as unknown as (string | null | null);

const b1: number | null | null = null as unknown as (number | null | null);
const b2: number | null | null = null as unknown as (number | null | null);
const b3: number | null | null = null as unknown as (number | null | null);
const b4: number | null | null = null as unknown as (number | null | null);

const c1: boolean | null | null = null as unknown as (boolean | null | null);
const c2: boolean | null | null = null as unknown as (boolean | null | null);
const c3: boolean | null | null = null as unknown as (boolean | null | null);
const c4: boolean | null | null = null as unknown as (boolean | null | null);

interface I { a: string }
const d1: I | null | null = null as unknown as (I | null | null);
const d2: I | null | null = null as unknown as (I | null | null);
const d3: I | null | null = null as unknown as (I | null | null);
const d4: I | null | null = null as unknown as (I | null | null);

const aa1 = a1 ?? 'whatever';
const aa2 = a2 ?? 'whatever';
const aa3 = a3 ?? 'whatever';
const aa4 = a4 ?? 'whatever';

const bb1 = b1 ?? 1;
const bb2 = b2 ?? 1;
const bb3 = b3 ?? 1;
const bb4 = b4 ?? 1;

const cc1 = c1 ?? true;
const cc2 = c2 ?? true;
const cc3 = c3 ?? true;
const cc4 = c4 ?? true;

const dd1 = d1 ?? {b: 1};
const dd2 = d2 ?? {b: 1};
const dd3 = d3 ?? {b: 1};
const dd4 = d4 ?? {b: 1};

// Repro from #34635

function foo(): void { }

const maybeBool = false;

if (!(maybeBool ?? true)) {
    foo();
}

if (maybeBool ?? true) {
    foo();
}
else {
    foo();
}

if (false ?? true) {
    foo();
}
else {
    foo();
}


function main(): void {}
