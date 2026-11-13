import { Base, Published } from "@test/data";
interface Local extends Base<number> { own: number; }
function main(): void {
    const local = JSON.parse('{"inherited":3,"own":4}') as Local;
    const published = JSON.parse('{"inherited":5,"own":6}') as Published;
    assert(local.inherited === 3 && published.inherited === 5);
    assert(local.optional === undefined);
    let rejected = 0;
    try { const bad = JSON.parse('{"inherited":"bad","own":4}') as Local; }
    catch (error: Error) { rejected++; }
    try { const bad = JSON.parse('{"own":4}') as Published; }
    catch (error: Error) { rejected++; }
    assert(rejected === 2);
}
