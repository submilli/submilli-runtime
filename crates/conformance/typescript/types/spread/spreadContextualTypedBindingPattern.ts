// @target: es2015
// #18308
interface Person {
  naam: string,
  age: number
}

const bob: Person = null as unknown as (Person);
const alice: Person = null as unknown as (Person);

// [ts] Initializer provides no value for this binding element and the binding element has no default value.
const { naam, age } = {...bob, ...alice}


function main(): void {}
