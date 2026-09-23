type N<T> = {value:T; next:N<T | number>|null;};
class Link {value:number=1;next:Link|null=null;}
class Parent {value:unknown=null;reset(value:unknown):void{this.value=value;}}
class Child extends Parent {value:N<number> ={value:1,next:null};}
export function main():void {const link=new Link();link.next=link;const c=new Child();c.reset(link);const valid=c.value;assert(valid.value===1);}
