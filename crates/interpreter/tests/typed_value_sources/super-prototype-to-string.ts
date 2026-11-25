class MyErr extends Error {
    constructor(msg: string) {
        super(msg);
        this.name = "MyErr";
    }
    override toString(): string {
        return "wrapped(" + super.toString() + ")";
    }
}

class SubErr extends MyErr {
    override toString(): string {
        return "sub(" + super.toString() + ")";
    }
}

class MyTypeErr extends TypeError {
    override toString(): string {
        return "mytype(" + super.toString() + ")";
    }
}

class Plain {}

class SubPlain extends Plain {
    override toString(): string {
        return "plain(" + super.toString() + ")";
    }
}

function main(): string {
    const out: string[] = [];
    out.push(new MyErr("boom").toString());
    out.push(new MyErr("").toString());
    out.push(new SubErr("boom").toString());
    out.push(new MyTypeErr("bad").toString());
    out.push(new SubPlain().toString());
    return out.join("|");
}
