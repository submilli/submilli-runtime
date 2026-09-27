// @target: es2015
// @strict: true
// members N and M of types S and T have the same name, same accessibility, same optionality, and N is assignable M
// additional optional properties do not cause errors

class S implements S2 { foo: string; }
class T implements T2 { foo: string; }
/*pruned*/;                       
/*pruned*/;                       

interface S2 { foo: string; bar?: string }
interface T2 { foo: string; baz?: string }
let s2: S2 = null as unknown as (S2);
let t2: T2 = null as unknown as (T2);

let a: { foo: string; bar?: string } = null as unknown as ({ foo: string; bar?: string });
let b: { foo: string; baz?: string } = null as unknown as ({ foo: string; baz?: string });

let a2: S2 = { foo: '' };
let b2: T2 = { foo: '' };

/**/; 
/**/; 
/**/;  
/**/;  

s2 = t2;
t2 = s2;
/**/;  
s2 = b;
s2 = a2;

a = b;
b = a;
/**/; 
a = s2;
a = a2;

a2 = b2;
b2 = a2;
a2 = b;
a2 = t2;
/**/;  


function main(): void {}
