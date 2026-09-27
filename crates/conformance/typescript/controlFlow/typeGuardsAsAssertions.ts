// @target: es2015
// @strictNullChecks: true

// Repro from #8513

let cond: boolean = null as unknown as (boolean);

export type Optional<a> = Some<a> | None;

export interface None { readonly none: string; }
export interface Some<a> { readonly some: a; }

export const none : None = { none: '' };

export function isSome<a>(value: Optional<a>): value is Some<a> {
    return 'some' in value;
}

function someFrom<a>(some: a): { some: a; } {
    return { some };
}

export function fn<r>(makeSome: () => r): void {
    let result: Optional<r> = none;
    result;  // None
    while (cond) {
        result;  // Some<r> | None
        result = someFrom(isSome(result) ? result.some : makeSome());
        result;  // Some<r>
    }
}

function foo1(): void {
    let x: string | number | boolean = 0;
    x;  // number
    while (cond) {
        x;  // number, then string | number
        x = typeof x === "string" ? x.slice() : "abc";
        x;  // string
    }
    x;
}

function foo2(): void {
    let x: string | number | boolean = 0;
    x;  // number
    while (cond) {
        x;  // number, then string | number
        if (typeof x === "string") {
            x = x.slice();
        }
        else {
            x = "abc";
        }
        x;  // string
    }
    x;
}

// Type guards as assertions

function f1(): void {
    let x: string | number | null = null;
    x;  // undefined
    if (x) {
        x;  // string | number (guard as assertion)
    }
    x;  // string | number | undefined
}

function f2(): void {
    let x: string | number | null = null;
    x;  // undefined
    if (typeof x === "string") {
        x;  // string (guard as assertion)
    }
    x;  // string | undefined
}

function f3(): void {
    let x: string | number | null = null;
    x;  // undefined
    if (!x) {
        return;
    }
    x;  // string | number (guard as assertion)
}

function f4(): void {
    let x: string | number | null = null;
    x;  // undefined
    if (typeof x === "boolean") {
        x;  // nothing (boolean not in declared type)
    }
    x;  // undefined
}

function f5(x: string | number): void {
    if (typeof x === "string" && typeof x === "number") {
        x;  // number (guard as assertion)
    }
    else {
        x;  // string | number
    }
    x;  // string | number
}

function f6(): void {
    let x: string | null | null = null as unknown as (string | null | null);
    x!.slice();
    x = "";
    x!.slice();
    x = null;
    x!.slice();
    x = null;
    x!.slice();
    x = <null | null>null;
    x!.slice();
    x = <string | null>"";
    x!.slice();
    x = <string | null>"";
    x!.slice();
}

function f7(): void {
    let x: string = null as unknown as (string);
    x!.slice();
}


function main(): void {}
