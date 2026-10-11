// @target: es2015
// these operators require their operands to be of type Any, the Number primitive type, or
// an enum type
enum E { a, b, c }

/*pruned*/;                           
let b: boolean = null as unknown as (boolean);
let c: number = null as unknown as (number);
let d: string = null as unknown as (string);
let e: { a: number } = null as unknown as ({ a: number });
/*pruned*/;                                 

// All of the below should be an error unless otherwise noted
// operator *
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r1b2 = b * b;
let r1b3 = b * c;
let r1b4 = b * d;
let r1b5 = b * e;
/*pruned*/;      

/*pruned*/;       //ok
let r1c2 = c * b;
let r1c3 = c * c; //ok
let r1c4 = c * d;
let r1c5 = c * e;
/*pruned*/;      

/*pruned*/;      
let r1d2 = d * b;
let r1d3 = d * c;
let r1d4 = d * d;
let r1d5 = d * e;
/*pruned*/;      

/*pruned*/;      
let r1e2 = e * b;
let r1e3 = e * c;
let r1e4 = e * d;
let r1e5 = e * e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r1g2 = E.a * b;
let r1g3 = E.a * c; //ok
let r1g4 = E.a * d;
let r1g5 = E.a * e;
/*pruned*/;        

/*pruned*/;         //ok
let r1h2 = b * E.b;
let r1h3 = c * E.b; //ok
let r1h4 = d * E.b;
let r1h5 = e * E.b;
/*pruned*/;        

// operator /
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r2b2 = b / b;
let r2b3 = b / c;
let r2b4 = b / d;
let r2b5 = b / e;
/*pruned*/;      

/*pruned*/;       //ok
let r2c2 = c / b;
let r2c3 = c / c; //ok
let r2c4 = c / d;
let r2c5 = c / e;
/*pruned*/;      

/*pruned*/;      
let r2d2 = d / b;
let r2d3 = d / c;
let r2d4 = d / d;
let r2d5 = d / e;
/*pruned*/;      

/*pruned*/;      
let r2e2 = e / b;
let r2e3 = e / c;
let r2e4 = e / d;
let r2e5 = e / e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r2g2 = E.a / b;
let r2g3 = E.a / c; //ok
let r2g4 = E.a / d;
let r2g5 = E.a / e;
/*pruned*/;        

/*pruned*/;         //ok
let r2h2 = b / E.b;
let r2h3 = c / E.b; //ok
let r2h4 = d / E.b;
let r2h5 = e / E.b;
/*pruned*/;        

// operator %
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r3b2 = b % b;
let r3b3 = b % c;
let r3b4 = b % d;
let r3b5 = b % e;
/*pruned*/;      

/*pruned*/;       //ok
let r3c2 = c % b;
let r3c3 = c % c; //ok
let r3c4 = c % d;
let r3c5 = c % e;
/*pruned*/;      

/*pruned*/;      
let r3d2 = d % b;
let r3d3 = d % c;
let r3d4 = d % d;
let r3d5 = d % e;
/*pruned*/;      

/*pruned*/;      
let r3e2 = e % b;
let r3e3 = e % c;
let r3e4 = e % d;
let r3e5 = e % e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r3g2 = E.a % b;
let r3g3 = E.a % c; //ok
let r3g4 = E.a % d;
let r3g5 = E.a % e;
/*pruned*/;        

/*pruned*/;         //ok
let r3h2 = b % E.b;
let r3h3 = c % E.b; //ok
let r3h4 = d % E.b;
let r3h5 = e % E.b;
/*pruned*/;        

// operator -
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r4b2 = b - b;
let r4b3 = b - c;
let r4b4 = b - d;
let r4b5 = b - e;
/*pruned*/;      

/*pruned*/;       //ok
let r4c2 = c - b;
let r4c3 = c - c; //ok
let r4c4 = c - d;
let r4c5 = c - e;
/*pruned*/;      

/*pruned*/;      
let r4d2 = d - b;
let r4d3 = d - c;
let r4d4 = d - d;
let r4d5 = d - e;
/*pruned*/;      

/*pruned*/;      
let r4e2 = e - b;
let r4e3 = e - c;
let r4e4 = e - d;
let r4e5 = e - e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r4g2 = E.a - b;
let r4g3 = E.a - c; //ok
let r4g4 = E.a - d;
let r4g5 = E.a - e;
/*pruned*/;        

/*pruned*/;         //ok
let r4h2 = b - E.b;
let r4h3 = c - E.b; //ok
let r4h4 = d - E.b;
let r4h5 = e - E.b;
/*pruned*/;        

// operator <<
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;       
let r5b2 = b << b;
let r5b3 = b << c;
let r5b4 = b << d;
let r5b5 = b << e;
/*pruned*/;       

/*pruned*/;        //ok
let r5c2 = c << b;
let r5c3 = c << c; //ok
let r5c4 = c << d;
let r5c5 = c << e;
/*pruned*/;       

/*pruned*/;       
let r5d2 = d << b;
let r5d3 = d << c;
let r5d4 = d << d;
let r5d5 = d << e;
/*pruned*/;       

/*pruned*/;       
let r5e2 = e << b;
let r5e3 = e << c;
let r5e4 = e << d;
let r5e5 = e << e;
/*pruned*/;       

/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;          //ok
let r5g2 = E.a << b;
let r5g3 = E.a << c; //ok
let r5g4 = E.a << d;
let r5g5 = E.a << e;
/*pruned*/;         

/*pruned*/;          //ok
let r5h2 = b << E.b;
let r5h3 = c << E.b; //ok
let r5h4 = d << E.b;
let r5h5 = e << E.b;
/*pruned*/;         

// operator >>
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;       
let r6b2 = b >> b;
let r6b3 = b >> c;
let r6b4 = b >> d;
let r6b5 = b >> e;
/*pruned*/;       

/*pruned*/;        //ok
let r6c2 = c >> b;
let r6c3 = c >> c; //ok
let r6c4 = c >> d;
let r6c5 = c >> e;
/*pruned*/;       

/*pruned*/;       
let r6d2 = d >> b;
let r6d3 = d >> c;
let r6d4 = d >> d;
let r6d5 = d >> e;
/*pruned*/;       

/*pruned*/;       
let r6e2 = e >> b;
let r6e3 = e >> c;
let r6e4 = e >> d;
let r6e5 = e >> e;
/*pruned*/;       

/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;          //ok
let r6g2 = E.a >> b;
let r6g3 = E.a >> c; //ok
let r6g4 = E.a >> d;
let r6g5 = E.a >> e;
/*pruned*/;         

/*pruned*/;          //ok
let r6h2 = b >> E.b;
let r6h3 = c >> E.b; //ok
let r6h4 = d >> E.b;
let r6h5 = e >> E.b;
/*pruned*/;         

// operator >>>
/*pruned*/;         //ok
/*pruned*/;        
/*pruned*/;         //ok
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        

/*pruned*/;        
let r7b2 = b >>> b;
let r7b3 = b >>> c;
let r7b4 = b >>> d;
let r7b5 = b >>> e;
/*pruned*/;        

/*pruned*/;         //ok
let r7c2 = c >>> b;
let r7c3 = c >>> c; //ok
let r7c4 = c >>> d;
let r7c5 = c >>> e;
/*pruned*/;        

/*pruned*/;        
let r7d2 = d >>> b;
let r7d3 = d >>> c;
let r7d4 = d >>> d;
let r7d5 = d >>> e;
/*pruned*/;        

/*pruned*/;        
let r7e2 = e >>> b;
let r7e3 = e >>> c;
let r7e4 = e >>> d;
let r7e5 = e >>> e;
/*pruned*/;        

/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        
/*pruned*/;        

/*pruned*/;           //ok
let r7g2 = E.a >>> b;
let r7g3 = E.a >>> c; //ok
let r7g4 = E.a >>> d;
let r7g5 = E.a >>> e;
/*pruned*/;          

/*pruned*/;           //ok
let r7h2 = b >>> E.b;
let r7h3 = c >>> E.b; //ok
let r7h4 = d >>> E.b;
let r7h5 = e >>> E.b;
/*pruned*/;          

// operator &
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r8b2 = b & b;
let r8b3 = b & c;
let r8b4 = b & d;
let r8b5 = b & e;
/*pruned*/;      

/*pruned*/;       //ok
let r8c2 = c & b;
let r8c3 = c & c; //ok
let r8c4 = c & d;
let r8c5 = c & e;
/*pruned*/;      

/*pruned*/;      
let r8d2 = d & b;
let r8d3 = d & c;
let r8d4 = d & d;
let r8d5 = d & e;
/*pruned*/;      

/*pruned*/;      
let r8e2 = e & b;
let r8e3 = e & c;
let r8e4 = e & d;
let r8e5 = e & e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r8g2 = E.a & b;
let r8g3 = E.a & c; //ok
let r8g4 = E.a & d;
let r8g5 = E.a & e;
/*pruned*/;        

/*pruned*/;         //ok
let r8h2 = b & E.b;
let r8h3 = c & E.b; //ok
let r8h4 = d & E.b;
let r8h5 = e & E.b;
/*pruned*/;        

// operator ^
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;       //ok
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;      
let r9b2 = b ^ b;
let r9b3 = b ^ c;
let r9b4 = b ^ d;
let r9b5 = b ^ e;
/*pruned*/;      

/*pruned*/;       //ok
let r9c2 = c ^ b;
let r9c3 = c ^ c; //ok
let r9c4 = c ^ d;
let r9c5 = c ^ e;
/*pruned*/;      

/*pruned*/;      
let r9d2 = d ^ b;
let r9d3 = d ^ c;
let r9d4 = d ^ d;
let r9d5 = d ^ e;
/*pruned*/;      

/*pruned*/;      
let r9e2 = e ^ b;
let r9e3 = e ^ c;
let r9e4 = e ^ d;
let r9e5 = e ^ e;
/*pruned*/;      

/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      
/*pruned*/;      

/*pruned*/;         //ok
let r9g2 = E.a ^ b;
let r9g3 = E.a ^ c; //ok
let r9g4 = E.a ^ d;
let r9g5 = E.a ^ e;
/*pruned*/;        

/*pruned*/;         //ok
let r9h2 = b ^ E.b;
let r9h3 = c ^ E.b; //ok
let r9h4 = d ^ E.b;
let r9h5 = e ^ E.b;
/*pruned*/;        

// operator |
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;        //ok
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;       
let r10b2 = b | b;
let r10b3 = b | c;
let r10b4 = b | d;
let r10b5 = b | e;
/*pruned*/;       

/*pruned*/;        //ok
let r10c2 = c | b;
let r10c3 = c | c; //ok
let r10c4 = c | d;
let r10c5 = c | e;
/*pruned*/;       

/*pruned*/;       
let r10d2 = d | b;
let r10d3 = d | c;
let r10d4 = d | d;
let r10d5 = d | e;
/*pruned*/;       

/*pruned*/;       
let r10e2 = e | b;
let r10e3 = e | c;
let r10e4 = e | d;
let r10e5 = e | e;
/*pruned*/;       

/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       
/*pruned*/;       

/*pruned*/;          //ok
let r10g2 = E.a | b;
let r10g3 = E.a | c; //ok
let r10g4 = E.a | d;
let r10g5 = E.a | e;
/*pruned*/;         

/*pruned*/;          //ok
let r10h2 = b | E.b;
let r10h3 = c | E.b; //ok
let r10h4 = d | E.b;
let r10h5 = e | E.b;
/*pruned*/;         

function main(): void {}
