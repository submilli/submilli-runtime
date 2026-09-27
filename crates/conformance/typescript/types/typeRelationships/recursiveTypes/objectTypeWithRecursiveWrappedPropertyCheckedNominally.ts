// @target: es2015
// Types with infinitely expanding recursive types are type checked nominally

class List<T> {
    data: T;
    next: List<List<T>>;
}

class MyList<T> {
    data: T;
    next: MyList<MyList<T>>;
}

let list1 = new List<number>();
let list2 = new List<string>();

let myList1 = new MyList<number>();
let myList2 = new MyList<string>();

list1 = myList1; // error, not nominally equal
list1 = myList2; // error, type mismatch

list2 = myList1; // error, not nominally equal
list2 = myList2; // error, type mismatch

let rList1 = new List<List<number>>();
let rMyList1 = new List<MyList<number>>();
rList1 = rMyList1; // error, not nominally equal

/*pruned*/;                                                                       
                   
                   

                                                            
                                                                
                
                   
                   
                
 

/*pruned*/;                                                             
                   
                                                                         

                                                            
                                                                

                   
                   
                
                
 

function main(): void {}
