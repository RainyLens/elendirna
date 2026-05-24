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

// write — same-origin이라 브라우저가 Origin/Sec-Fetch를 자동 부착(서버 가드 통과). [[N0106]] P2
async function sendJSON(method, path, body) {
  const r = await fetch(BASE + path, {
    method,
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify(body),
  });
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

const id_ = (id) => encodeURIComponent(id);

export const api = {
  entries: () => getJSON("/entries"),
  entry: (id) => getJSON("/entries/" + id_(id)),
  bundle: (id) => getJSON("/entries/" + id_(id) + "/bundle"),
  lineage: (id) => getJSON("/lineage/" + id_(id)),
  search: (q) => getJSON("/search?title_contains=" + encodeURIComponent(q)),
  validate: () => getJSON("/validate"),

  // write
  createRevision: (id, body) => sendJSON("POST", "/entries/" + id_(id) + "/revisions", body),
  createEntry: (body) => sendJSON("POST", "/entries", body),
  setStatus: (id, status) => sendJSON("PUT", "/entries/" + id_(id) + "/status", { status }),
  setTags: (id, tags) => sendJSON("PUT", "/entries/" + id_(id) + "/tags", { tags }),
  addLink: (id, to) => sendJSON("POST", "/entries/" + id_(id) + "/links", { to }),
};
