export interface Alpha { f: number; a: string; }
export interface Beta { f: number; b: string; }
export class CA { f: number = 1; }
export class CB { f: number = 2; }

export interface Box<T> { f: number; item: T; }
export interface Other { f: number; z: string; }

export interface DA { kind: "a"; f: number; extra: string; }
export interface DB { kind: "b"; f: number; other: string; }

export function makeShape(): Alpha | Beta { return { f: 1, a: "x" }; }
export function makeClassy(): CA | CB { return new CA(); }
export function makeBoxed(): Box<Alpha> | Other { return { f: 1, item: { f: 2, a: "y" } }; }
export function makeStructural(): { f: number; item: Alpha } | Other {
    return { f: 1, item: { f: 2, a: "y" } };
}
export function makeDiscriminated(): DA | DB { return { kind: "a", f: 1, extra: "x" }; }
