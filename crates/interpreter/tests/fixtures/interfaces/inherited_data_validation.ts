interface Named { name: string; }
interface Generic<T> { value: T; optional?: T; }
type Other = { enabled: boolean; name: string; };
interface Pet<T> extends Generic<T>, Other { age: number; }
interface Narrow extends Generic<number | string> { value: number; }
interface Optional extends Partial<Named> { age: number; }

function main(): void {
    const pet = JSON.parse('{"name":"Milo","value":4,"enabled":true,"age":2}') as Pet<number>;
    assert(pet.name === "Milo" && pet.value === 4 && pet.enabled);
    let rejected = 0;
    try { const bad = JSON.parse('{"name":3,"value":4,"enabled":true,"age":2}') as Pet<number>; }
    catch (error: Error) { rejected++; }
    try { const bad = JSON.parse('{"name":"Milo","value":"wrong","enabled":true,"age":2}') as Pet<number>; }
    catch (error: Error) { rejected++; }
    try { const bad = JSON.parse('{"value":4,"enabled":true,"age":2}') as Pet<number>; }
    catch (error: Error) { rejected++; }
    try { const bad = JSON.parse('{"value":"wrong"}') as Narrow; }
    catch (error: Error) { rejected++; }
    assert(rejected === 4);
    const optional = JSON.parse('{"age":2}') as Optional;
    assert(optional.name === undefined);
}
