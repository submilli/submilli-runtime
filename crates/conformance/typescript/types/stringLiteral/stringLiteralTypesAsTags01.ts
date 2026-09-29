// @target: es2015
// @strict: true
// @declaration: true

type Kind = "A" | "B"

interface Entity {
    kind: Kind;
}

interface A extends Entity {
    kind: "A";
    a: number;
}

interface B extends Entity {
    kind: "B";
    b: string;
}

/*pruned*/;                                              
/*pruned*/;                                              
/*pruned*/;                                                    
function hasKind(entity: Entity, kind: Kind): boolean {
    return entity.kind === kind;
}

let x: A = {
    kind: "A",
    a: 100,
}

if (hasKind(x, "A")) {
    let a = x;
}
else {
    let b = x;
}

if (!hasKind(x, "B")) {
    let c = x;
}
else {
    let d = x;
}

function main(): void {}
