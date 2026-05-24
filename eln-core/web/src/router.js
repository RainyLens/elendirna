// 경량 hash 라우터 — react-router 미도입. cross-ref(`#/entry/N####`)와 동일 규칙.
//   #/entries          → 랜딩(목록)
//   #/entry/N####      → entry bundle 뷰
//   #/lineage/N####    → lineage 뷰
export function parseHash() {
  const h = window.location.hash.replace(/^#\/?/, "");
  const [seg, id] = h.split("/");
  if (seg === "entry" && id) return { view: "entry", id };
  if (seg === "lineage" && id) return { view: "lineage", id };
  return { view: "entries" };
}

export function useRoute() {
  const { useState, useEffect } = React;
  const [route, setRoute] = useState(parseHash());
  useEffect(() => {
    const on = () => setRoute(parseHash());
    window.addEventListener("hashchange", on);
    return () => window.removeEventListener("hashchange", on);
  }, []);
  return route;
}

export function go(hash) {
  window.location.hash = hash;
}
