// @target: es2015
// @strict: true

let optionalProperties: { k1?: string } = null as unknown as ({ k1?: string });
let undefinedProperties: { k1: string | null } = null as unknown as ({ k1: string | null });

let stringDictionary: { [key: string]: string } = null as unknown as ({ [key: string]: string });
stringDictionary = optionalProperties;  // ok
stringDictionary = undefinedProperties; // error

/*pruned*/
/*pruned*/
/*pruned*/

/*pruned*/
/*pruned*/

/*pruned*/
/*pruned*/
/*pruned*/
/*pruned*/


function main(): void {}
