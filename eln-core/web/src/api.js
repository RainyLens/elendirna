// /api read-only 클라이언트. 모든 엔드포인트는 GET·JSON.
const BASE = "/api";

async function getJSON(path) {
  const r = await fetch(BASE + path, { headers: { accept: "application/json" } });
  if (!r.ok) {
    let msg = `${r.status} ${r.statusText}`;
    try {
      const b = await r.json();
      if (b && b.message) msg = b.message;
    } catch (_) {}
    const err = new Error(msg);
    err.status = r.status;
    throw err;
  }
  return r.json();
}

export const api = {
  entries: () => getJSON("/entries"),
  entry: (id) => getJSON("/entries/" + encodeURIComponent(id)),
  bundle: (id) => getJSON("/entries/" + encodeURIComponent(id) + "/bundle"),
  lineage: (id) => getJSON("/lineage/" + encodeURIComponent(id)),
  search: (q) => getJSON("/search?title_contains=" + encodeURIComponent(q)),
  validate: () => getJSON("/validate"),
};
