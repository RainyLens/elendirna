// 엔트리 — React(UMD 전역)로 #root에 App 마운트.
import { App } from "./app.jsx";

const root = ReactDOM.createRoot(document.getElementById("root"));
root.render(<App />);
