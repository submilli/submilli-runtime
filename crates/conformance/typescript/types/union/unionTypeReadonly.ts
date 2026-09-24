// @target: es2015
interface Base {
    readonly value: number;
}
interface Identical {
    readonly value: number;
}
interface Mutable {
    value: number;
}
interface DifferentType {
    readonly value: string;
}
interface DifferentName {
    readonly other: number;
}
let base: Base = null as unknown as (Base);
base.value = 12 // error, lhs can't be a readonly property
let identical: Base | Identical = null as unknown as (Base | Identical);
identical.value = 12; // error, lhs can't be a readonly property
let mutable: Base | Mutable = null as unknown as (Base | Mutable);
mutable.value = 12; // error, lhs can't be a readonly property
let differentType: Base | DifferentType = null as unknown as (Base | DifferentType);
differentType.value = 12; // error, lhs can't be a readonly property
let differentName: Base | DifferentName = null as unknown as (Base | DifferentName);
differentName.value = 12; // error, property 'value' doesn't exist



function main(): void {}
