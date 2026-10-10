function effects(): number { let i: number = 0; let s=0; if(i !== null) { while(i++ < 3) { s += i; } return s+i; } return -1; }
function main(): void { assert(effects()===10, "effects"); }
