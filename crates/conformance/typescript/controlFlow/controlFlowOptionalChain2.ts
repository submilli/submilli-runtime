// @target: es2015
// @strictNullChecks: true

type A = {
  type: 'A';
  name: string;
}

type B = {
  type: 'B';
}

function funcTwo(arg: A | B | undefined): void {
  if (arg?.type === 'B') {
    arg; // `B`
    return;
  }

  arg;
  arg?.name;
}

function funcThree(arg: A | B | null): void {
  if (arg?.type === 'B') {
    arg; // `B`
    return;
  }

  arg;
  arg?.name;
}

type U = { kind: undefined, u: 'u' }
type N = { kind: null, n: 'n' }
type X = { kind: 'X', x: 'x' }

function f1(x: X | U | undefined): void {
    if (x?.kind === undefined) {
        x; // U | undefined
    }
    else {
        x; // X
    }
}

function f2(x: X | N | undefined): void {
    if (x?.kind === undefined) {
        x; // undefined
    }
    else {
        x; // X | N
    }
}

function f3(x: X | U | null): void {
    if (x?.kind === undefined) {
        x; // U | null
    }
    else {
        x; // X
    }
}

function f4(x: X | N | null): void {
    if (x?.kind === undefined) {
        x; // null
    }
    else {
        x; // X | N
    }
}

function f5(x: X | U | undefined): void {
    if (x?.kind === null) {
        x; // never
    }
    else {
        x; // X | U | undefined
    }
}

function f6(x: X | N | undefined): void {
    if (x?.kind === null) {
        x; // N
    }
    else {
        x; // X | undefined
    }
}

function f7(x: X | U | null): void {
    if (x?.kind === null) {
        x; // never
    }
    else {
        x; // X | U | null
    }
}

function f8(x: X | N | null): void {
    if (x?.kind === null) {
        x; // N
    }
    else {
        x; // X | null
    }
}


function main(): void {}
