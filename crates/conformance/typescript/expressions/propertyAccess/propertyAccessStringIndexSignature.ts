// @target: es2015
// @noImplicitAny: true, false
interface Flags { [name: string]: boolean };
let flags: Flags = null as unknown as (Flags);
flags.b;
flags.f;
flags.isNotNecessarilyNeverFalse;
flags['this is fine'];

interface Empty { }
let empty: Empty = null as unknown as (Empty);
empty.nope;
empty["that's ok"];


function main(): void {}
