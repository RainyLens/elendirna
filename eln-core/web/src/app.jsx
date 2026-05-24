// 앱 셸 — hash 라우팅으로 뷰 전환. TopChrome + 스크롤 본문.
import { useRoute } from "./router.js";
import { TopChrome } from "./atoms.jsx";
import { EntriesLanding } from "./entries.jsx";
import { EntryView } from "./entry.jsx";
import { LineageView } from "./lineage.jsx";

export function App() {
  const route = useRoute();
  let body;
  if (route.view === "entry") body = <EntryView id={route.id} />;
  else if (route.view === "lineage") body = <LineageView id={route.id} />;
  else body = <EntriesLanding />;

  return (
    <div className="app theme theme-light">
      <TopChrome section="entries" />
      <div className="app-body">{body}</div>
    </div>
  );
}
