// @target: es2015
// @strict: true

// Verify that properties can vary independently in comparable relationship

const x: { a: 1, b: string } = null as unknown as ({ a: 1, b: string });
const y: { a: number, b: 'a' } = null as unknown as ({ a: number, b: 'a' });

x === y;


function main(): void {}
