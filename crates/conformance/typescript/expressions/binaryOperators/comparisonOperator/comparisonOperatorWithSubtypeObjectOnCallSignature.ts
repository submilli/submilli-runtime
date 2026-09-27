// @target: es2015
class Base {
    public a: string;
}

class Derived extends Base {
    public b: string;
}

let a1: { fn(): void } = null as unknown as ({ fn(): void });
let b1: { fn(): void } = null as unknown as ({ fn(): void });

let a2: { fn(a: number, b: string): void } = null as unknown as ({ fn(a: number, b: string): void });
let b2: { fn(a: number, b: string): void } = null as unknown as ({ fn(a: number, b: string): void });

let a3: { fn(a: number, b: string): void } = null as unknown as ({ fn(a: number, b: string): void });
let b3: { fn(a: number): void } = null as unknown as ({ fn(a: number): void });

let a4: { fn(a: number, b: string): void } = null as unknown as ({ fn(a: number, b: string): void });
let b4: { fn(): void } = null as unknown as ({ fn(): void });

let a5: { fn(a: Base): void } = null as unknown as ({ fn(a: Base): void });
let b5: { fn(a: Derived): void } = null as unknown as ({ fn(a: Derived): void });

let a6: { fn(a: Derived, b: Base): void } = null as unknown as ({ fn(a: Derived, b: Base): void });
let b6: { fn(a: Base, b: Derived): void } = null as unknown as ({ fn(a: Base, b: Derived): void });

let a7: { fn(): void } = null as unknown as ({ fn(): void });
let b7: { fn(): Base } = null as unknown as ({ fn(): Base });

let a8: { fn(): Base } = null as unknown as ({ fn(): Base });
let b8: { fn(): Base } = null as unknown as ({ fn(): Base });

let a9: { fn(): Base } = null as unknown as ({ fn(): Base });
let b9: { fn(): Derived } = null as unknown as ({ fn(): Derived });

/*pruned*/;                                                                   
/*pruned*/;                                                                         

let a11: { fn(...a: Base[]): void } = null as unknown as ({ fn(...a: Base[]): void });
let b11: { fn(...a: Derived[]): void } = null as unknown as ({ fn(...a: Derived[]): void });

//var a12: { fn<T, U extends T>(t: T, u: U): T[] };
//var b12: { fn<A, B extends A>(a: A, b: B): A[] };

// operator <
let r1a1 = a1 < b1;
let r1a2 = a2 < b2;
let r1a3 = a3 < b3;
let r1a4 = a4 < b4;
let r1a5 = a5 < b5;
let r1a6 = a6 < b6;
let r1a7 = a7 < b7;
let r1a8 = a8 < b8;
let r1a9 = a9 < b9;
/*pruned*/;           
let r1a11 = a11 < b11;
//var r1a12 = a12 < b12;

let r1b1 = b1 < a1;
let r1b2 = b2 < a2;
let r1b3 = b3 < a3;
let r1b4 = b4 < a4;
let r1b5 = b5 < a5;
let r1b6 = b6 < a6;
let r1b7 = b7 < a7;
let r1b8 = b8 < a8;
let r1b9 = b9 < a9;
/*pruned*/;           
let r1b11 = b11 < a11;
//var r1b12 = b12 < a12;

// operator >
let r2a1 = a1 > b1;
let r2a2 = a2 > b2;
let r2a3 = a3 > b3;
let r2a4 = a4 > b4;
let r2a5 = a5 > b5;
let r2a6 = a6 > b6;
let r2a7 = a7 > b7;
let r2a8 = a8 > b8;
let r2a9 = a9 > b9;
/*pruned*/;           
let r2a11 = a11 > b11;
//var r2a12 = a12 > b12;

let r2b1 = b1 > a1;
let r2b2 = b2 > a2;
let r2b3 = b3 > a3;
let r2b4 = b4 > a4;
let r2b5 = b5 > a5;
let r2b6 = b6 > a6;
let r2b7 = b7 > a7;
let r2b8 = b8 > a8;
let r2b9 = b9 > a9;
/*pruned*/;           
let r2b11 = b11 > a11;
//var r2b12 = b12 > a12;

// operator <=
let r3a1 = a1 <= b1;
let r3a2 = a2 <= b2;
let r3a3 = a3 <= b3;
let r3a4 = a4 <= b4;
let r3a5 = a5 <= b5;
let r3a6 = a6 <= b6;
let r3a7 = a7 <= b7;
let r3a8 = a8 <= b8;
let r3a9 = a9 <= b9;
/*pruned*/;            
let r3a11 = a11 <= b11;
//var r3a12 = a12 <= b12;

let r3b1 = b1 <= a1;
let r3b2 = b2 <= a2;
let r3b3 = b3 <= a3;
let r3b4 = b4 <= a4;
let r3b5 = b5 <= a5;
let r3b6 = b6 <= a6;
let r3b7 = b7 <= a7;
let r3b8 = b8 <= a8;
let r3b9 = b9 <= a9;
/*pruned*/;            
let r3b11 = b11 <= a11;
//var r3b12 = b12 <= a12;

// operator >=
let r4a1 = a1 >= b1;
let r4a2 = a2 >= b2;
let r4a3 = a3 >= b3;
let r4a4 = a4 >= b4;
let r4a5 = a5 >= b5;
let r4a6 = a6 >= b6;
let r4a7 = a7 >= b7;
let r4a8 = a8 >= b8;
let r4a9 = a9 >= b9;
/*pruned*/;            
let r4a11 = a11 >= b11;
//var r4a12 = a12 >= b12;

let r4b1 = b1 >= a1;
let r4b2 = b2 >= a2;
let r4b3 = b3 >= a3;
let r4b4 = b4 >= a4;
let r4b5 = b5 >= a5;
let r4b6 = b6 >= a6;
let r4b7 = b7 >= a7;
let r4b8 = b8 >= a8;
let r4b9 = b9 >= a9;
/*pruned*/;            
let r4b11 = b11 >= a11;
//var r4b12 = b12 >= a12;

// operator ==
let r5a1 = a1 == b1;
let r5a2 = a2 == b2;
let r5a3 = a3 == b3;
let r5a4 = a4 == b4;
let r5a5 = a5 == b5;
let r5a6 = a6 == b6;
let r5a7 = a7 == b7;
let r5a8 = a8 == b8;
let r5a9 = a9 == b9;
/*pruned*/;            
let r5a11 = a11 == b11;
//var r5a12 = a12 == b12;

let r5b1 = b1 == a1;
let r5b2 = b2 == a2;
let r5b3 = b3 == a3;
let r5b4 = b4 == a4;
let r5b5 = b5 == a5;
let r5b6 = b6 == a6;
let r5b7 = b7 == a7;
let r5b8 = b8 == a8;
let r5b9 = b9 == a9;
/*pruned*/;            
let r5b11 = b11 == a11;
//var r5b12 = b12 == a12;

// operator !=
let r6a1 = a1 != b1;
let r6a2 = a2 != b2;
let r6a3 = a3 != b3;
let r6a4 = a4 != b4;
let r6a5 = a5 != b5;
let r6a6 = a6 != b6;
let r6a7 = a7 != b7;
let r6a8 = a8 != b8;
let r6a9 = a9 != b9;
/*pruned*/;            
let r6a11 = a11 != b11;
//var r6a12 = a12 != b12;

let r6b1 = b1 != a1;
let r6b2 = b2 != a2;
let r6b3 = b3 != a3;
let r6b4 = b4 != a4;
let r6b5 = b5 != a5;
let r6b6 = b6 != a6;
let r6b7 = b7 != a7;
let r6b8 = b8 != a8;
let r6b9 = b9 != a9;
/*pruned*/;            
let r6b11 = b11 != a11;
//var r6b12 = b12 != a12;

// operator ===
let r7a1 = a1 === b1;
let r7a2 = a2 === b2;
let r7a3 = a3 === b3;
let r7a4 = a4 === b4;
let r7a5 = a5 === b5;
let r7a6 = a6 === b6;
let r7a7 = a7 === b7;
let r7a8 = a8 === b8;
let r7a9 = a9 === b9;
/*pruned*/;             
let r7a11 = a11 === b11;
//var r7a12 = a12 === b12;

let r7b1 = b1 === a1;
let r7b2 = b2 === a2;
let r7b3 = b3 === a3;
let r7b4 = b4 === a4;
let r7b5 = b5 === a5;
let r7b6 = b6 === a6;
let r7b7 = b7 === a7;
let r7b8 = b8 === a8;
let r7b9 = b9 === a9;
/*pruned*/;             
let r7b11 = b11 === a11;
//var r7b12 = b12 === a12;

// operator !==
let r8a1 = a1 !== b1;
let r8a2 = a2 !== b2;
let r8a3 = a3 !== b3;
let r8a4 = a4 !== b4;
let r8a5 = a5 !== b5;
let r8a6 = a6 !== b6;
let r8a7 = a7 !== b7;
let r8a8 = a8 !== b8;
let r8a9 = a9 !== b9;
/*pruned*/;             
let r8a11 = a11 !== b11;
//var r8a12 = a12 !== b12;

let r8b1 = b1 !== a1;
let r8b2 = b2 !== a2;
let r8b3 = b3 !== a3;
let r8b4 = b4 !== a4;
let r8b5 = b5 !== a5;
let r8b6 = b6 !== a6;
let r8b7 = b7 !== a7;
let r8b8 = b8 !== a8;
let r8b9 = b9 !== a9;
/*pruned*/;             
let r8b11 = b11 !== a11;
//var r8b12 = b12 !== a12;

function main(): void {}
