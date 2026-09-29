// @target: es2015
// @strict: true
interface Props {
    readonly a?: string
    readonly b?: string
    readonly c?: string
    readonly d?: string
    readonly e?: string
    readonly f?: string
    readonly g?: string
    readonly h?: string
    readonly i?: string
    readonly j?: string
    readonly k?: string
    readonly l?: string
    readonly m?: string
    readonly n?: string
    readonly o?: string
    readonly p?: string
    readonly q?: string
    readonly r?: string
    readonly s?: string
    readonly t?: string
    readonly u?: string
    readonly v?: string
    readonly w?: string
    readonly x?: string
    readonly y?: string
    readonly z?: string
}

function parseWithSpread(config: Record<string, number>): Props {
    return {
        ...config.a !== null && { a: config.a.toString() },
        ...config.b !== null && { b: config.b.toString() },
        ...config.c !== null && { c: config.c.toString() },
        ...config.d !== null && { d: config.d.toString() },
        ...config.e !== null && { e: config.e.toString() },
        ...config.f !== null && { f: config.f.toString() },
        ...config.g !== null && { g: config.g.toString() },
        ...config.h !== null && { h: config.h.toString() },
        ...config.i !== null && { i: config.i.toString() },
        ...config.j !== null && { j: config.j.toString() },
        ...config.k !== null && { k: config.k.toString() },
        ...config.l !== null && { l: config.l.toString() },
        ...config.m !== null && { m: config.m.toString() },
        ...config.n !== null && { n: config.n.toString() },
        ...config.o !== null && { o: config.o.toString() },
        ...config.p !== null && { p: config.p.toString() },
        ...config.q !== null && { q: config.q.toString() },
        ...config.r !== null && { r: config.r.toString() },
        ...config.s !== null && { s: config.s.toString() },
        ...config.t !== null && { t: config.t.toString() },
        ...config.u !== null && { u: config.u.toString() },
        ...config.v !== null && { v: config.v.toString() },
        ...config.w !== null && { w: config.w.toString() },
        ...config.x !== null && { x: config.x.toString() },
        ...config.y !== null && { y: config.y.toString() },
        ...config.z !== null && { z: config.z.toString() }
    }
}

parseWithSpread({ a: 1, b: 2, z: 26 })

function main(): void {}
