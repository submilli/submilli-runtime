// @target: es2015
// @strict: true
// @declaration: true

// Should all be strings.
let [hello, brave, newish, world] = ["Hello", "Brave", "New", "World"];

type RexOrRaptor = "t-rex" | "raptor"
let [im, a, dinosaur]: ["I'm", "a", RexOrRaptor] = ['I\'m', 'a', 't-rex'];

rawr(dinosaur);

function rawr(dino: RexOrRaptor): "ROAAAAR!" | "yip yip!" {
    if (dino === "t-rex") {
        return "ROAAAAR!";
    }
    if (dino === "raptor") {
        return "yip yip!";
    }

    throw "Unexpected " + dino;
}

function main(): void {}
