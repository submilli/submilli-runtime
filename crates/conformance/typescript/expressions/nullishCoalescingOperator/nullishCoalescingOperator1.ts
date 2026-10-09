// @target: es2015
// @strict: true
// @allowUnreachableCode: false

const a1: string | undefined | null = null as unknown as (string | undefined | null);
const a2: string | undefined | null = null as unknown as (string | undefined | null);
const a3: string | undefined | null = null as unknown as (string | undefined | null);
const a4: string | undefined | null = null as unknown as (string | undefined | null);

const b1: number | undefined | null = null as unknown as (number | undefined | null);
const b2: number | undefined | null = null as unknown as (number | undefined | null);
const b3: number | undefined | null = null as unknown as (number | undefined | null);
const b4: number | undefined | null = null as unknown as (number | undefined | null);

const c1: boolean | undefined | null = null as unknown as (boolean | undefined | null);
const c2: boolean | undefined | null = null as unknown as (boolean | undefined | null);
const c3: boolean | undefined | null = null as unknown as (boolean | undefined | null);
const c4: boolean | undefined | null = null as unknown as (boolean | undefined | null);

interface I { a: string }
const d1: I | undefined | null = null as unknown as (I | undefined | null);
const d2: I | undefined | null = null as unknown as (I | undefined | null);
const d3: I | undefined | null = null as unknown as (I | undefined | null);
const d4: I | undefined | null = null as unknown as (I | undefined | null);

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
