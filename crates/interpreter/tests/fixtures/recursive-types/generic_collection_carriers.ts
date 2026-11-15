interface MapNode { m: Map<string, MapNode> }
interface SetNode { s: Set<SetNode> }
type Link = { i: LinkedNode };
interface LinkedNode { s: Set<Link> }
function count<T>(values: T[]): number { return values.length; }
function main(): void {
    assert(count<MapNode>([]) === 0);
    assert(count<SetNode>([]) === 0);
    assert(count<Link>([]) === 0);
    const mapNode: MapNode = { m: new Map<string, MapNode>() };
    mapNode.m.set("self", mapNode);
    const setNode: SetNode = { s: new Set<SetNode>() };
    setNode.s.add(setNode);
    assert(count<MapNode>([mapNode]) === 1);
    assert(count<SetNode>([setNode]) === 1);
}
