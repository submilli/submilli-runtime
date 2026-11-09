// @target: es2015
// @strict: true

let optionalProperties: { k1?: string } = null as unknown as ({ k1?: string });
let undefinedProperties: { k1: string | undefined } = null as unknown as ({ k1: string | undefined });

let stringDictionary: { [key: string]: string } = null as unknown as ({ [key: string]: string });
stringDictionary = optionalProperties;  // ok
stringDictionary = undefinedProperties; // error

let probablyArray: { [key: number]: string } = null as unknown as ({ [key: number]: string });
let numberLiteralKeys: { 1?: string } = null as unknown as ({ 1?: string });
probablyArray = numberLiteralKeys;  // error

let optionalUndefined: { k1?: undefined } = null as unknown as ({ k1?: undefined });
let dict: { [key: string]: string } = optionalUndefined; // error

function f<T>(): void {
	let optional: { k1?: T } = undefined!;
	let dict: { [key: string]: T | number } = optional; // ok
}


function main(): void {}
