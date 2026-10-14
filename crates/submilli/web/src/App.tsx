// The page's top: the signed-out screen, the first load, or the page itself.
import { SIGNED_OUT_LINE } from "./api/login";
import type { PageStore } from "./store/store";
import { useStore } from "./store/useStore";
import { Inline } from "./ui/controls";
import { Page } from "./ui/Page";
import { LOADING, SIGNED_OUT_WHY, loadFailed } from "./ui/wording";

function Screen({ line, why }: { line: string; why?: string }) {
  return (
    <div className="pg pg-screen">
      <div className="pg-brand">
        Submilli <span>Playground</span>
      </div>
      {why && <p>{why}</p>}
      <p>
        <Inline text={line} />
      </p>
    </div>
  );
}

export function App({ store }: { store: PageStore }) {
  const snap = useStore(store);
  if (snap.auth === "signed-out") return <Screen why={SIGNED_OUT_WHY} line={SIGNED_OUT_LINE} />;
  // Until the first session's record is read, so the page never shows it empty first.
  if (!snap.data.status || (snap.loading && !snap.loadError)) return <Screen line={snap.loadError ? loadFailed(snap.loadError) : LOADING} />;
  return <Page store={store} snap={snap} />;
}
